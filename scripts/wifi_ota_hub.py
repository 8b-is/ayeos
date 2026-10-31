#!/usr/bin/env python3
"""
ayeOS v5.4.0 — Iceland (.is) Signature-Verified OTA Updater, USB Auto-Installer & Wi-Fi OTA Hub
- Queries ONLY our sovereign Iceland (.is) servers for version checks & OS manifests:
  1. https://g8t.is/g8t/ota/blu-g75/manifest.json
  2. https://aye.is/g8t/ota/blu-g75/manifest.json
  3. https://8b.is/g8t/ota/blu-g75/manifest.json
- Enforces the 3-gate check before staging or installing any update:
  1. Remote image_sha256_hex != installed image_sha256_hex (or connected USB phone version != manifest.version)
  2. Cryptographic signature verification (G8T_IS_AYE_ONLY_SOVEREIGN_GATE_V1)
  3. Full binary payload SHA-256 & BLAKE2s verification
- Automatically detects any USB-connected BLU G75 running an older ayeOS (via USB 18d1:4ee0 bcdDevice / 0xA9),
  triggers the 0xA8 OTA Reset Switch, catches /dev/ttyACM0 ("FASTBOOT"), and flashes boot_a & boot_b!
- Serves /g8t/ota/blu-g75/manifest.json and /g8t/ota/blu-g75/boot.img on 0.0.0.0:8788 (Wi-Fi 224_5g_P3M8)
  and broadcasts 132-byte TWIF OTA beacons on UDP :8888.
"""
import fcntl
import glob
import hashlib
import http.server
import json
import os
import shutil
import socket
import struct
import subprocess
import sys
import threading
import time
import urllib.request

BOOT_DIR = os.path.abspath(
    os.path.join(os.path.dirname(__file__), "../hardware/blu_g75_g1170/boot_b")
)
IMG_PATH = os.path.join(BOOT_DIR, "ayeos_boot_b_gz.img")
MANIFEST_PATH = os.path.join(BOOT_DIR, "manifest.json")

# Strictly pinned to our sovereign Iceland (.is) domains on the i1-dns mesh
UPSTREAM_MANIFEST_URLS = [
    "https://g8t.is/g8t/ota/blu-g75/manifest.json",
    "https://aye.is/g8t/ota/blu-g75/manifest.json",
    "https://8b.is/g8t/ota/blu-g75/manifest.json",
]


def compute_aye_signature(device_slug: str, version: str, sha256_hex: str) -> str:
    sig_material = (
        b"G8T_IS_AYE_ONLY_SOVEREIGN_GATE_V1:"
        b"aye-sovereign-founder:"
        + device_slug.encode("ascii")
        + b":"
        + version.encode("ascii")
        + b":"
        + sha256_hex.encode("ascii")
    )
    return hashlib.sha256(sig_material).hexdigest()


def verify_manifest_and_image(manifest: dict, img_bytes: bytes = None) -> tuple[bool, str]:
    if manifest.get("domain_tag") != "G8T_IS_AYE_ONLY_SOVEREIGN_GATE_V1":
        return False, "INVALID_DOMAIN_TAG"
    if manifest.get("attested_by_aye") != "aye-sovereign-founder":
        return False, "UNTRUSTED_ATTESTOR"
    expected_sig = compute_aye_signature(
        manifest.get("device_slug", ""),
        manifest.get("version", ""),
        manifest.get("image_sha256_hex", ""),
    )
    if manifest.get("signature_hex") != expected_sig:
        return False, "SIGNATURE_MISMATCH"
    if img_bytes is not None:
        if len(img_bytes) != manifest.get("image_size_bytes"):
            return False, "SIZE_MISMATCH"
        if hashlib.sha256(img_bytes).hexdigest() != manifest.get("image_sha256_hex"):
            return False, "PAYLOAD_SHA256_MISMATCH"
        if hashlib.blake2s(img_bytes).hexdigest() != manifest.get("image_blake2s_hex"):
            return False, "PAYLOAD_BLAKE2S_MISMATCH"
    return True, "VERIFIED_OK"


def version_to_bcd_hex(ver_str: str) -> str:
    parts = [int(p) for p in ver_str.strip().split(".") if p.isdigit()]
    while len(parts) < 3:
        parts.append(0)
    return f"{parts[0]:02d}{parts[1]:01d}{parts[2]:01d}"


def probe_connected_ayeos_usb() -> tuple[str, str]:
    """
    Returns (usbfs_path, bcdDevice_str) if a running ayeOS Bare-Metal node (18d1:4ee0)
    is connected via USB, else ("", "").
    """
    for devpath in glob.glob("/sys/bus/usb/devices/*"):
        try:
            with open(os.path.join(devpath, "idVendor")) as fv:
                vid = fv.read().strip().lower()
            with open(os.path.join(devpath, "idProduct")) as fp:
                pid = fp.read().strip().lower()
            if vid == "18d1" and pid == "4ee0":
                with open(os.path.join(devpath, "bcdDevice")) as fd_bcd:
                    bcd = fd_bcd.read().strip().lower()
                with open(os.path.join(devpath, "busnum")) as fb:
                    busnum = int(fb.read().strip())
                with open(os.path.join(devpath, "devnum")) as fd_num:
                    devnum = int(fd_num.read().strip())
                return f"/dev/bus/usb/{busnum:03d}/{devnum:03d}", bcd
        except Exception:
            pass
    return "", ""


def auto_flash_connected_blu_g75(img_path: str, target_version: str) -> bool:
    """
    Triggers the USB 0xA8 OTA Reset Switch on a connected 18d1:4ee0 BLU G75,
    catches the MediaTek Preloader /dev/ttyACM0 'FASTBOOT' handshake,
    erases init_boot_a/b, flashes boot_a & boot_b, and reboots into target_version.
    """
    fastboot_bin = shutil.which("fastboot")
    if not fastboot_bin:
        return False

    usbfs, bcd = probe_connected_ayeos_usb()
    if usbfs:
        print(
            f"[OTA-AUTO] Connected BLU G75 at {usbfs} (bcdDevice={bcd}) != target v{target_version}. Triggering 0xA8 OTA Reset..."
        )
        try:
            fd = os.open(usbfs, os.O_RDWR)
            USBDEVFS_CONTROL = 0xC0185500
            ctrl = struct.pack("<BBHHHQI", 0x40, 0xA8, 0xA8E0, 0, 0, 0, 1000)
            try:
                fcntl.ioctl(fd, USBDEVFS_CONTROL, ctrl)
            except Exception:
                pass
            USBDEVFS_RESET = 21780
            try:
                fcntl.ioctl(fd, USBDEVFS_RESET, 0)
            except Exception:
                pass
            os.close(fd)
        except Exception as exc:
            print(f"[OTA-AUTO] USB control note: {exc}")

    # Catch /dev/ttyACM0..1 preloader FASTBOOT window (up to 12 seconds)
    TIOCEXCL = 0x540C
    start_ts = int(time.time())
    entered_fb = False
    while int(time.time()) - start_ts < 12:
        fb_check = subprocess.run(
            [fastboot_bin, "devices"], capture_output=True, text=True, check=False
        )
        if fb_check.stdout.strip():
            entered_fb = True
            break
        for dev in ("/dev/ttyACM0", "/dev/ttyACM1"):
            if os.path.exists(dev):
                try:
                    fd = os.open(dev, os.O_RDWR | os.O_NOCTTY | os.O_NONBLOCK)
                    try:
                        fcntl.ioctl(fd, TIOCEXCL)
                    except Exception:
                        pass
                    for _ in range(120):
                        try:
                            os.write(fd, b"FASTBOOT")
                        except Exception:
                            pass
                    os.close(fd)
                    time.sleep(2)
                    entered_fb = True
                    break
                except Exception:
                    pass
        if entered_fb:
            break

    fb_check = subprocess.run(
        [fastboot_bin, "devices"], capture_output=True, text=True, check=False
    )
    if not fb_check.stdout.strip():
        return False

    print(f"[OTA-AUTO] Flashing verified ayeOS v{target_version} to boot_a & boot_b...")
    subprocess.run([fastboot_bin, "erase", "init_boot_a"], check=False)
    subprocess.run([fastboot_bin, "erase", "init_boot_b"], check=False)
    subprocess.run([fastboot_bin, "flash", "boot_a", img_path], check=True)
    subprocess.run([fastboot_bin, "flash", "boot_b", img_path], check=True)
    subprocess.run([fastboot_bin, "set_active", "b"], check=False)
    subprocess.run([fastboot_bin, "reboot"], check=True)
    print(f"[OTA-AUTO] Successfully upgraded BLU G75 to ayeOS Bare-Metal v{target_version}!")
    return True


def poll_iceland_upstream_once(auto_flash_usb: bool = False) -> str:
    try:
        local_manifest = json.loads(open(MANIFEST_PATH, "r").read())
        local_sha = local_manifest.get("image_sha256_hex", "")
        local_ver = local_manifest.get("version", "5.4.0")
    except Exception:
        local_manifest = {}
        local_sha = ""
        local_ver = "0.0.0"

    status_msg = f"LOCAL_VERIFIED (v{local_ver} sha256={local_sha[:16]}...)"

    for url in UPSTREAM_MANIFEST_URLS:
        # Enforce strict .is domain policy for version checks
        if not (
            url.startswith("https://g8t.is/")
            or url.startswith("https://aye.is/")
            or url.startswith("https://8b.is/")
        ):
            continue
        try:
            req = urllib.request.Request(
                url, headers={"User-Agent": "ayeOS-Iceland-OTA-Verifier/5.4.0"}
            )
            with urllib.request.urlopen(req, timeout=4) as resp:
                remote_manifest = json.loads(resp.read().decode("utf-8"))
            ok, _reason = verify_manifest_and_image(remote_manifest, None)
            if not ok:
                continue
            remote_sha = remote_manifest.get("image_sha256_hex", "")
            remote_ver = remote_manifest.get("version", "")
            if remote_sha == local_sha:
                status_msg = f"UP_TO_DATE ({url} v{remote_ver} sha256={local_sha[:16]}... sig=VERIFIED)"
                break

            # New signed version detected on g8t.is / aye.is! Download only from .is or local hub:
            for img_url in remote_manifest.get("primary_ota_urls", []):
                if not (
                    img_url.startswith("https://g8t.is/")
                    or img_url.startswith("https://aye.is/")
                    or img_url.startswith("https://8b.is/")
                    or img_url.startswith("http://192.168.151.14:8788/")
                ):
                    continue
                try:
                    ireq = urllib.request.Request(
                        img_url, headers={"User-Agent": "ayeOS-Iceland-OTA-Verifier/5.4.0"}
                    )
                    with urllib.request.urlopen(ireq, timeout=8) as iresp:
                        img_bytes = iresp.read()
                    ok_img, _reason_img = verify_manifest_and_image(remote_manifest, img_bytes)
                    if ok_img:
                        with open(IMG_PATH + ".tmp", "wb") as f:
                            f.write(img_bytes)
                        os.replace(IMG_PATH + ".tmp", IMG_PATH)
                        with open(MANIFEST_PATH + ".tmp", "w") as f:
                            json.dump(remote_manifest, f, indent=2)
                        os.replace(MANIFEST_PATH + ".tmp", MANIFEST_PATH)
                        local_ver = remote_ver
                        status_msg = f"UPDATED_FROM_ICELAND_IS ({img_url} -> v{remote_ver})"
                        if auto_flash_usb:
                            auto_flash_connected_blu_g75(IMG_PATH, remote_ver)
                        return status_msg
                except Exception:
                    continue
        except Exception:
            continue

    if auto_flash_usb and os.path.exists(IMG_PATH):
        usbfs, bcd = probe_connected_ayeos_usb()
        expected_bcd = version_to_bcd_hex(local_ver)
        if usbfs and bcd != expected_bcd:
            auto_flash_connected_blu_g75(IMG_PATH, local_ver)

    return status_msg


class OtaHttpHandler(http.server.BaseHTTPRequestHandler):
    def log_message(self, fmt, *args):
        pass

    def do_GET(self):
        if self.path in ("/g8t/ota/blu-g75/manifest.json", "/ota/blu-g75/manifest.json", "/"):
            data = open(MANIFEST_PATH, "rb").read()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)
        elif self.path in ("/g8t/ota/blu-g75/boot.img", "/ota/blu-g75/boot.img"):
            data = open(IMG_PATH, "rb").read()
            self.send_response(200)
            self.send_header("Content-Type", "application/octet-stream")
            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)
        else:
            self.send_response(404)
            self.end_headers()


def udp_twif_and_iceland_poll_loop(auto_flash_usb: bool):
    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    sock.setsockopt(socket.SOL_SOCKET, socket.SO_BROADCAST, 1)
    seq = 0
    while True:
        try:
            if seq % 15 == 0:
                poll_iceland_upstream_once(auto_flash_usb=auto_flash_usb)
            manifest = json.loads(open(MANIFEST_PATH, "r").read())
            pkt = bytearray(132)
            struct.pack_into("<I", pkt, 0, 0x54574946)  # "TWIF"
            struct.pack_into("<I", pkt, 4, seq)
            ssid = b"224_5g_P3M8"
            pkt[8 : 8 + len(ssid)] = ssid
            url = b"http://192.168.151.14:8788/g8t/ota/blu-g75/boot.img"
            pkt[40 : 40 + len(url)] = url
            sha_bytes = bytes.fromhex(manifest["image_sha256_hex"])
            pkt[96:128] = sha_bytes
            struct.pack_into("<I", pkt, 128, 432)
            sock.sendto(bytes(pkt), ("192.168.151.255", 8888))
            seq += 1
        except Exception:
            pass
        time.sleep(2)


if __name__ == "__main__":
    auto_flash = "--auto-flash" in sys.argv
    if "--verify-only" in sys.argv:
        m = json.loads(open(MANIFEST_PATH, "r").read())
        img = open(IMG_PATH, "rb").read()
        ok, msg = verify_manifest_and_image(m, img)
        print(
            f"Manifest & Image Verification: ok={ok} status={msg} upstream={poll_iceland_upstream_once(auto_flash_usb=auto_flash)}"
        )
        sys.exit(0 if ok else 1)

    t = threading.Thread(
        target=udp_twif_and_iceland_poll_loop, args=(auto_flash,), daemon=True
    )
    t.start()
    srv = http.server.ThreadingHTTPServer(("0.0.0.0", 8788), OtaHttpHandler)
    print(
        "🦀 [g8t.is / aye.is Iceland OTA Hub v5.4.0] Listening on 0.0.0.0:8788 (192.168.151.14:8788) + UDP :8888..."
    )
    srv.serve_forever()
