#!/usr/bin/env python3
"""AyeOS 5.6 Live Hardware Debug Socket Server & CLI Client (ADBG v1).

Zero floating-point arithmetic. Linux stdlib only.
Bridges UNIX domain socket (/tmp/ayeos-debug.sock) and TCP (127.0.0.1:4328)
to the AyeOS bare-metal USB EP0 debugger on BLU G75 (18d1:4ee0 bcdDevice=0562):
  - (0xc0, 0xb9) / (0xc0, 0xba): ADBG 64-byte live PMIC, SmartPA, MMIO, SPI0,
    continuous synth hold, and touch/piano/tracker injection
  - (0xc0, 0xb8): ATRK ScreamTracker & 24-key Piano playback
  - (0xc0, 0xa8, wValue=0xa8e0): AOTA remote reboot into fastboot
"""
import argparse
import json
import os
from pathlib import Path
import select
import socket
import struct
import sys
import time

sys.path.insert(0, str(Path(__file__).resolve().parent))

import capture_board4 as b4
import capture_tone as ct
import screamtracker_jam as jam

DEFAULT_UNIX_SOCK = "/tmp/ayeos-debug.sock"
DEFAULT_TCP_HOST = "127.0.0.1"
DEFAULT_TCP_PORT = 4328


def sleep_ms(ms):
    if ms > 0:
        select.poll().poll(int(ms))


class DebugUsb(ct.Usb):
    ALLOWED = ct.Usb.ALLOWED + (
        (0xc0, 0xb8),
        (0xc0, 0xb9),
        (0xc0, 0xba),
        (0xc0, 0xa8),
    )


def decode_adbg(packet):
    if len(packet) != 64 or packet[:4] != b"ADBG":
        raise ValueError("invalid ADBG response packet")
    (
        seq,
        last_op,
        last_idx,
        status,
        result,
        dbg_arg,
        pending,
        touch_en,
        touch_x,
        touch_y,
        touch_events,
    ) = struct.unpack_from("<IHHIIIBBHHH", packet, 4)
    buf = packet[32:64]
    words = list(struct.unpack("<8I", buf))
    u16s = list(struct.unpack("<16H", buf))
    return {
        "protocol": "ayeos-adbg-v1",
        "seq": seq,
        "last_op": f"0x{last_op:04x}",
        "last_idx": f"0x{last_idx:04x}",
        "status": status,
        "status_hex": f"0x{status:04x}",
        "result": result,
        "result_hex": f"0x{result:08x}",
        "dbg_arg_hex": f"0x{dbg_arg:08x}",
        "touch": {
            "last_x": touch_x,
            "last_y": touch_y,
            "events": touch_events,
            "enabled": bool(touch_en),
        },
        "pending": bool(pending),
        "buf_hex": buf.hex(),
        "buf_u16_hex": [f"0x{v:04x}" for v in u16s],
        "buf_u32_hex": [f"0x{w:08x}" for w in words],
    }


def open_debug_usb(device_path=None):
    path = device_path or jam.find_tone_usb_device()
    if not path:
        raise RuntimeError("No AyeOS audio-tone1 USB device (18d1:4ee0 bcdDevice=0562) found")
    usb = DebugUsb(path, 60)
    return usb, path


def set_arg32(usb, val32):
    lo = val32 & 0xFFFF
    hi = (val32 >> 16) & 0xFFFF
    usb.read(0xc0, 0xb9, 0x0000, lo, 64)
    usb.read(0xc0, 0xb9, 0x0001, hi, 64)


def set_addr32(usb, addr32):
    lo = addr32 & 0xFFFF
    hi = (addr32 >> 16) & 0xFFFF
    usb.read(0xc0, 0xb9, 0x000a, lo, 64)
    usb.read(0xc0, 0xb9, 0x000b, hi, 64)


def exec_debug_op(usb, op_val, idx=0, timeout_ms=2500, initial_wait_ms=25):
    before = decode_adbg(usb.read(0xc0, 0xba, 0, 0, 64))
    target_seq = (before["seq"] + 1) & 0xFFFFFFFF
    usb.read(0xc0, 0xb9, op_val, idx, 64)
    sleep_ms(initial_wait_ms)
    deadline_ns = time.monotonic_ns() + int(timeout_ms) * 1_000_000
    while time.monotonic_ns() < deadline_ns:
        try:
            after = decode_adbg(usb.read(0xc0, 0xba, 0, 0, 64))
        except OSError:
            sleep_ms(80)
            continue
        if after["seq"] == target_seq and not after["pending"]:
            return after
        sleep_ms(80)
    return decode_adbg(usb.read(0xc0, 0xba, 0, 0, 64))


def execute_request(req, device_path=None):
    cmd = str(req.get("cmd", "status")).strip().lower()
    usb, path = open_debug_usb(device_path)
    try:
        if cmd == "status":
            adbg = decode_adbg(usb.read(0xc0, 0xba, 0, 0, 64))
            atrk = jam.decode_atrk(usb.read(0xc0, 0xb8, 0, 0, 64))
            return {"ok": True, "device": path, "adbg": adbg, "tracker": atrk}

        if cmd == "pmic_read":
            reg = int(str(req.get("reg", "0x2408")), 0) & 0xFFFF
            window = 1 if req.get("window", True) else 0
            res = exec_debug_op(usb, (window << 8) | 2, reg)
            out = {
                "ok": res["status"] == 1,
                "reg": f"0x{reg:04x}",
                "value": f"0x{res['result'] & 0xFFFF:04x}",
                "adbg": res,
            }
            if window:
                out["window_8_even_regs"] = {
                    f"0x{(reg + i * 2) & 0xFFFF:04x}": res["buf_u16_hex"][i]
                    for i in range(8)
                }
            return out

        if cmd == "pmic_write":
            reg = int(str(req["reg"]), 0) & 0xFFFF
            val = int(str(req["val"]), 0) & 0xFFFF
            mask = int(str(req.get("mask", "0xffff")), 0) & 0xFFFF
            usb.read(0xc0, 0xb9, 0x0000, val, 64)
            usb.read(0xc0, 0xb9, 0x0001, mask, 64)
            res = exec_debug_op(usb, 3, reg)
            return {
                "ok": res["status"] == 1,
                "reg": f"0x{reg:04x}",
                "written": f"0x{val:04x}",
                "mask": f"0x{mask:04x}",
                "readback": f"0x{res['result'] & 0xFFFF:04x}",
                "adbg": res,
            }

        if cmd == "pa_read":
            addr = int(str(req.get("addr", "0x58")), 0) & 0xFF
            reg = int(str(req.get("reg", "0x00")), 0) & 0x7F
            window = 1 if req.get("window", True) else 0
            res = exec_debug_op(usb, (window << 8) | 4, (addr << 8) | reg)
            return {
                "ok": res["status"] == 1,
                "addr": f"0x{addr:02x}",
                "reg": f"0x{reg:02x}",
                "value": f"0x{res['result'] & 0xFF:02x}",
                "window_32_hex": res["buf_hex"] if window else None,
                "adbg": res,
            }

        if cmd == "pa_write":
            addr = int(str(req.get("addr", "0x58")), 0) & 0xFF
            reg = int(str(req["reg"]), 0) & 0x7F
            val = int(str(req["val"]), 0) & 0xFF
            usb.read(0xc0, 0xb9, 0x0000, val, 64)
            res = exec_debug_op(usb, 5, (addr << 8) | reg)
            return {
                "ok": res["status"] == 1,
                "addr": f"0x{addr:02x}",
                "reg": f"0x{reg:02x}",
                "written": f"0x{val:02x}",
                "readback": f"0x{res['result'] & 0xFF:02x}",
                "adbg": res,
            }

        if cmd == "mmio_read":
            addr = int(str(req["addr"]), 0) & 0xFFFFFFFF
            width = int(str(req.get("width", 32)), 0)
            count = max(1, min(8, int(str(req.get("count", 4)), 0)))
            sub = 1 if width == 8 else (2 if width == 16 else 0)
            set_addr32(usb, addr)
            res = exec_debug_op(usb, (sub << 8) | 6, count)
            return {
                "ok": res["status"] == 1,
                "addr": f"0x{addr:08x}",
                "width": width,
                "value": f"0x{res['result']:08x}",
                "words": {
                    f"0x{(addr + i * 4) & 0xFFFFFFFF:08x}": res["buf_u32_hex"][i]
                    for i in range(count)
                },
                "adbg": res,
            }

        if cmd == "mmio_write":
            addr = int(str(req["addr"]), 0) & 0xFFFFFFFF
            val = int(str(req["val"]), 0) & 0xFFFFFFFF
            width = int(str(req.get("width", 32)), 0)
            sub = 1 if width == 8 else (2 if width == 16 else 0)
            set_addr32(usb, addr)
            set_arg32(usb, val)
            res = exec_debug_op(usb, (sub << 8) | 7, 0)
            return {
                "ok": res["status"] == 1,
                "addr": f"0x{addr:08x}",
                "written": f"0x{val:08x}",
                "readback": f"0x{res['result']:08x}",
                "adbg": res,
            }

        if cmd == "spi_xfer":
            sub = int(str(req.get("sub", 0)), 0) & 0xFF
            tx0 = int(str(req.get("tx0", "0")), 0) & 0xFFFFFFFF
            tx1 = int(str(req.get("tx1", "0")), 0) & 0xFFFFFFFF
            tx2 = int(str(req.get("tx2", "0")), 0) & 0xFFFF
            set_arg32(usb, tx0)
            set_addr32(usb, tx1)
            res = exec_debug_op(usb, (sub << 8) | 8, tx2)
            return {
                "ok": res["status"] == 1,
                "sub": sub,
                "rx_words": res["buf_u32_hex"][:4],
                "spi_regs": res["buf_u32_hex"][4:],
                "adbg": res,
            }

        if cmd == "synth_start":
            div = max(1, min(31, int(str(req.get("div", 12)), 0)))
            gain_reg = int(str(req.get("gain_reg", "0x0000")), 0) & 0xFFFF
            spk_mask = int(str(req.get("spk_mask", 3)), 0) & 3
            usb.read(0xc0, 0xb9, 0x0000, gain_reg, 64)
            usb.read(0xc0, 0xb9, 0x0001, 0, 64)
            res = exec_debug_op(usb, (1 << 8) | 9, (spk_mask << 8) | div, timeout_ms=4000, initial_wait_ms=160)
            return {
                "ok": res["status"] == 1,
                "div": div,
                "approx_hz": 6000 // div,
                "gain_reg": f"0x{gain_reg:04x}",
                "spk_mask": spk_mask,
                "adbg": res,
            }

        if cmd == "synth_stop":
            res = exec_debug_op(usb, (2 << 8) | 9, 0, timeout_ms=3000, initial_wait_ms=100)
            return {
                "ok": res["status"] == 1,
                "off_mask": f"0x{res['result']:05x}",
                "verified_off": res["result"] == ct.OFF_MASK,
                "adbg": res,
            }

        if cmd == "touch":
            x = int(str(req["x"]), 0) & 0xFFFF
            y = int(str(req["y"]), 0) & 0xFFFF
            usb.read(0xc0, 0xb9, 0x0000, y, 64)
            res = exec_debug_op(usb, (0 << 8) | 9, x, timeout_ms=3500, initial_wait_ms=420)
            atrk = jam.decode_atrk(usb.read(0xc0, 0xb8, 0, 0, 64))
            hit_code = res["result"] & 0xFFFF
            hit_kind = "none"
            hit_index = None
            if (hit_code & 0xFF00) == 0x0100:
                hit_kind = "piano_key"
                hit_index = hit_code & 0xFF
            elif (hit_code & 0xFF00) == 0x0200:
                hit_kind = "tracker_row"
                hit_index = hit_code & 0xFF
            return {
                "ok": res["status"] == 1,
                "x": x,
                "y": y,
                "hit_kind": hit_kind,
                "hit_index": hit_index,
                "tracker": atrk,
                "adbg": res,
            }

        if cmd == "jam":
            val = int(str(req.get("val", "0x0080")), 0) & 0xFFFF
            idx = int(str(req.get("idx", "0")), 0) & 0xFFFF
            status = jam.trigger_and_wait(usb, val, idx, timeout_ms=5000)
            return {"ok": status["last_outcome"] == 1, "tracker": status}

        if cmd == "touch_probe":
            # Read SPI0 controller window (0x1100a000..0x1100a01c) + INFRA CG + GPIO_DIN0
            spi_res = exec_debug_op(usb, (0 << 8) | 8, 0)
            set_addr32(usb, 0x10001090)
            cg_res = exec_debug_op(usb, 6, 4)
            set_addr32(usb, 0x10005200)
            gpio_res = exec_debug_op(usb, 6, 6)
            return {
                "ok": True,
                "panel": "GC7272 720x1640@120Hz in-cell touch on spi0@1100a000",
                "spi0_regs_1100a000": spi_res["buf_u32_hex"],
                "infra_cg_10001090": cg_res["buf_u32_hex"][:4],
                "gpio_din_10005200": gpio_res["buf_u32_hex"][:6],
            }

        if cmd == "ota_reboot":
            pkt = usb.read(0xc0, 0xa8, 0xa8e0, 0, 64)
            return {
                "ok": pkt[:4] == b"AOTA",
                "magic": pkt[:4].decode("ascii", errors="replace"),
                "action": "rebooting_to_fastboot",
            }

        return {"ok": False, "error": f"unknown command: {cmd}"}
    finally:
        usb.close()


def parse_line_to_request(line):
    text = line.strip()
    if not text:
        return {"cmd": "status"}
    if text.startswith("{"):
        return json.loads(text)
    parts = text.split()
    cmd = parts[0].lower()
    if cmd == "status":
        return {"cmd": "status"}
    if cmd == "pmic_read":
        return {"cmd": "pmic_read", "reg": parts[1] if len(parts) > 1 else "0x2408"}
    if cmd == "pmic_write":
        return {
            "cmd": "pmic_write",
            "reg": parts[1],
            "val": parts[2],
            "mask": parts[3] if len(parts) > 3 else "0xffff",
        }
    if cmd == "pa_read":
        return {
            "cmd": "pa_read",
            "addr": parts[1] if len(parts) > 1 else "0x58",
            "reg": parts[2] if len(parts) > 2 else "0x00",
        }
    if cmd == "pa_write":
        return {"cmd": "pa_write", "addr": parts[1], "reg": parts[2], "val": parts[3]}
    if cmd == "mmio_read":
        return {
            "cmd": "mmio_read",
            "addr": parts[1],
            "count": parts[2] if len(parts) > 2 else "4",
        }
    if cmd == "mmio_write":
        return {"cmd": "mmio_write", "addr": parts[1], "val": parts[2]}
    if cmd == "spi_xfer":
        return {
            "cmd": "spi_xfer",
            "sub": parts[1] if len(parts) > 1 else "0",
            "tx0": parts[2] if len(parts) > 2 else "0",
            "tx1": parts[3] if len(parts) > 3 else "0",
        }
    if cmd == "synth_start":
        return {
            "cmd": "synth_start",
            "div": parts[1] if len(parts) > 1 else "12",
            "gain_reg": parts[2] if len(parts) > 2 else "0x0000",
            "spk_mask": parts[3] if len(parts) > 3 else "3",
        }
    if cmd == "synth_stop":
        return {"cmd": "synth_stop"}
    if cmd == "touch":
        return {"cmd": "touch", "x": parts[1], "y": parts[2]}
    if cmd == "jam":
        return {
            "cmd": "jam",
            "val": parts[1] if len(parts) > 1 else "0x0080",
            "idx": parts[2] if len(parts) > 2 else "0",
        }
    if cmd in ("touch_probe", "fdt_touch_info"):
        return {"cmd": "touch_probe"}
    if cmd == "ota_reboot":
        return {"cmd": "ota_reboot"}
    return {"cmd": cmd}


def serve_daemon(unix_path, tcp_host, tcp_port, device_path=None):
    if os.path.exists(unix_path):
        os.unlink(unix_path)
    unix_srv = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    unix_srv.bind(unix_path)
    os.chmod(unix_path, 0o666)
    unix_srv.listen(8)
    unix_srv.setblocking(False)

    tcp_srv = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    tcp_srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    tcp_srv.bind((tcp_host, tcp_port))
    tcp_srv.listen(8)
    tcp_srv.setblocking(False)

    poller = select.poll()
    poller.register(unix_srv.fileno(), select.POLLIN)
    poller.register(tcp_srv.fileno(), select.POLLIN)
    clients = {}
    buffers = {}

    print(
        json.dumps(
            {
                "daemon": "ayeos-debug-socket-v1",
                "unix_socket": unix_path,
                "tcp_socket": f"{tcp_host}:{tcp_port}",
                "status": "listening",
            }
        ),
        flush=True,
    )
    try:
        while True:
            events = poller.poll(250)
            for fd, flag in events:
                if fd == unix_srv.fileno():
                    conn, _ = unix_srv.accept()
                    conn.setblocking(False)
                    clients[conn.fileno()] = conn
                    buffers[conn.fileno()] = b""
                    poller.register(conn.fileno(), select.POLLIN)
                elif fd == tcp_srv.fileno():
                    conn, _ = tcp_srv.accept()
                    conn.setblocking(False)
                    clients[conn.fileno()] = conn
                    buffers[conn.fileno()] = b""
                    poller.register(conn.fileno(), select.POLLIN)
                elif fd in clients:
                    conn = clients[fd]
                    if flag & (select.POLLHUP | select.POLLERR):
                        poller.unregister(fd)
                        conn.close()
                        clients.pop(fd, None)
                        buffers.pop(fd, None)
                        continue
                    try:
                        chunk = conn.recv(4096)
                    except OSError:
                        chunk = b""
                    if not chunk:
                        poller.unregister(fd)
                        conn.close()
                        clients.pop(fd, None)
                        buffers.pop(fd, None)
                        continue
                    buffers[fd] += chunk
                    while b"\n" in buffers[fd]:
                        line_bytes, buffers[fd] = buffers[fd].split(b"\n", 1)
                        line = line_bytes.decode("utf-8", errors="replace")
                        try:
                            req = parse_line_to_request(line)
                            resp = execute_request(req, device_path=device_path)
                        except Exception as exc:
                            resp = {"ok": False, "error": str(exc)}
                        out_bytes = (json.dumps(resp) + "\n").encode("utf-8")
                        try:
                            conn.sendall(out_bytes)
                        except OSError:
                            break
    finally:
        for conn in clients.values():
            conn.close()
        unix_srv.close()
        tcp_srv.close()
        if os.path.exists(unix_path):
            os.unlink(unix_path)


def send_via_unix_socket(unix_path, req):
    sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    sock.connect(unix_path)
    try:
        payload = (json.dumps(req) + "\n").encode("utf-8")
        sock.sendall(payload)
        buf = b""
        while b"\n" not in buf:
            chunk = sock.recv(4096)
            if not chunk:
                break
            buf += chunk
        return json.loads(buf.decode("utf-8").strip())
    finally:
        sock.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--serve", action="store_true", help="Run background UNIX + TCP debug socket daemon")
    parser.add_argument("--unix-sock", default=DEFAULT_UNIX_SOCK, help="UNIX socket path")
    parser.add_argument("--tcp-host", default=DEFAULT_TCP_HOST, help="TCP bind host")
    parser.add_argument("--tcp-port", type=int, default=DEFAULT_TCP_PORT, help="TCP bind port")
    parser.add_argument("--device", help="Explicit /dev/bus/usb/BBB/DDD device path")
    parser.add_argument("--direct", action="store_true", help="Bypass socket daemon and talk to USB directly")
    parser.add_argument("command", nargs="*", help="Debug command (e.g. status, pmic_read 0x2408, touch 160 1100, ota_reboot)")
    args = parser.parse_args()

    if args.serve:
        serve_daemon(args.unix_sock, args.tcp_host, args.tcp_port, device_path=args.device)
        return

    cmd_line = " ".join(args.command) if args.command else "status"
    req = parse_line_to_request(cmd_line)
    if not args.direct and os.path.exists(args.unix_sock):
        try:
            resp = send_via_unix_socket(args.unix_sock, req)
            print(json.dumps(resp, indent=2))
            return
        except OSError:
            pass
    resp = execute_request(req, device_path=args.device)
    print(json.dumps(resp, indent=2))


if __name__ == "__main__":
    main()
