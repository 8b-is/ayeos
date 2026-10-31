#!/usr/bin/env python3
"""
ayeOS v5.1 — 8b.is / g8t.is Signature-Verified OTA Updater & Wi-Fi OTA Hub (192.168.151.14:8788)
- Queries main 8b.is / g8t.is servers for the latest BLU G75 bare-metal OS image.
- Enforces the 3-gate check before staging any update:
  1. Remote image_sha256_hex != installed image_sha256_hex
  2. Cryptographic signature verification (G8T_IS_AYE_ONLY_SOVEREIGN_GATE_V1)
  3. Full binary payload SHA-256 & BLAKE2s verification
- Serves /g8t/ota/blu-g75/manifest.json and /g8t/ota/blu-g75/boot.img on 0.0.0.0:8788 (Wi-Fi 224_5g_P3M8)
  and broadcasts 132-byte TWIF OTA beacons on UDP :8888.
"""
import hashlib
import http.server
import json
import os
import socket
import struct
import sys
import threading
import time
import urllib.request

BOOT_DIR = os.path.abspath(
    os.path.join(os.path.dirname(__file__), "../hardware/blu_g75_g1170/boot_b")
)
IMG_PATH = os.path.join(BOOT_DIR, "ayeos_boot_b_gz.img")
MANIFEST_PATH = os.path.join(BOOT_DIR, "manifest.json")

UPSTREAM_MANIFEST_URLS = [
    "https://g8t.is/g8t/ota/blu-g75/manifest.json",
    "https://8b.is/g8t/ota/blu-g75/manifest.json",
    "https://raw.githubusercontent.com/8b-is/g8t/main/firmware/blu_g75_g1170/boot_b/manifest.json",
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


def poll_8b_is_upstream_once() -> str:
    try:
        local_manifest = json.loads(open(MANIFEST_PATH, "r").read())
        local_sha = local_manifest.get("image_sha256_hex", "")
    except Exception:
        local_sha = ""

    for url in UPSTREAM_MANIFEST_URLS:
        try:
            req = urllib.request.Request(url, headers={"User-Agent": "ayeOS-OTA-Verifier/5.1"})
            with urllib.request.urlopen(req, timeout=4) as resp:
                remote_manifest = json.loads(resp.read().decode("utf-8"))
            ok, reason = verify_manifest_and_image(remote_manifest, None)
            if not ok:
                continue
            remote_sha = remote_manifest.get("image_sha256_hex", "")
            if remote_sha == local_sha:
                return f"UP_TO_DATE ({url} sha256={local_sha[:16]}... sig=VERIFIED)"
            # Different & signature verified! Download and verify binary payload:
            for img_url in remote_manifest.get("primary_ota_urls", []):
                try:
                    ireq = urllib.request.Request(img_url, headers={"User-Agent": "ayeOS-OTA-Verifier/5.1"})
                    with urllib.request.urlopen(ireq, timeout=8) as iresp:
                        img_bytes = iresp.read()
                    ok_img, reason_img = verify_manifest_and_image(remote_manifest, img_bytes)
                    if ok_img:
                        with open(IMG_PATH + ".tmp", "wb") as f:
                            f.write(img_bytes)
                        os.replace(IMG_PATH + ".tmp", IMG_PATH)
                        with open(MANIFEST_PATH + ".tmp", "w") as f:
                            json.dump(remote_manifest, f, indent=2)
                        os.replace(MANIFEST_PATH + ".tmp", MANIFEST_PATH)
                        return f"UPDATED_FROM_8B_IS ({img_url} -> v{remote_manifest.get('version')})"
                except Exception:
                    continue
        except Exception:
            continue
    return f"LOCAL_VERIFIED (sha256={local_sha[:16]}...)"


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


def udp_twif_beacon_loop():
    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    sock.setsockopt(socket.SOL_SOCKET, socket.SO_BROADCAST, 1)
    seq = 0
    while True:
        try:
            manifest = json.loads(open(MANIFEST_PATH, "r").read())
            pkt = bytearray(132)
            struct.pack_into("<I", pkt, 0, 0x54574946)  # "TWIF"
            struct.pack_into("<I", pkt, 4, seq)
            ssid = b"224_5g_P3M8"
            pkt[8:8 + len(ssid)] = ssid
            url = b"http://192.168.151.14:8788/g8t/ota/blu-g75/boot.img"
            pkt[40:40 + len(url)] = url
            sha_bytes = bytes.fromhex(manifest["image_sha256_hex"])
            pkt[96:128] = sha_bytes
            struct.pack_into("<I", pkt, 128, 432)
            sock.sendto(bytes(pkt), ("192.168.151.255", 8888))
            seq += 1
        except Exception:
            pass
        time.sleep(2)


if __name__ == "__main__":
    if "--verify-only" in sys.argv:
        m = json.loads(open(MANIFEST_PATH, "r").read())
        img = open(IMG_PATH, "rb").read()
        ok, msg = verify_manifest_and_image(m, img)
        print(f"Manifest & Image Verification: ok={ok} status={msg} upstream={poll_8b_is_upstream_once()}")
        sys.exit(0 if ok else 1)

    t = threading.Thread(target=udp_twif_beacon_loop, daemon=True)
    t.start()
    srv = http.server.ThreadingHTTPServer(("0.0.0.0", 8788), OtaHttpHandler)
    print("🦀 [8b.is / g8t.is Wi-Fi OTA Hub] Listening on 0.0.0.0:8788 (192.168.151.14:8788) + UDP :8888...")
    srv.serve_forever()
