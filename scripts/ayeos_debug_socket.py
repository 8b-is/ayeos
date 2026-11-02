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
DEFAULT_HTTP_PORT = 4380

QWAVE_STATE = {
    "scroll_y": 0,
    "active_tab": 0,
    "last_touch_x": 0,
    "last_touch_y": 0,
    "last_hit": "ready",
}

# MT6358_AFE_SGEN_CFG1 (0x22c2) bits 3:0 are a frequency multiplier N = 1..15
# where Frequency = (8000 Hz / 64) * N = 125 Hz * N.
# (N = 0 or N = 16 has low 4 bits = 0 -> 0 Hz DC click with no tone!)
# Map all 14 white keys (indices 0,2,4,5,7,9,11,12,14,16,17,19,21,23) to distinct
# ascending multipliers N = 1..14 (125 Hz, 250 Hz, 375 Hz, ..., 1750 Hz)!
SEMITONE_HZ = (
    262, 277, 294, 311, 330, 349, 370, 392, 415, 440, 466, 494,
    523, 554, 587, 622, 659, 698, 740, 784, 831, 880, 932, 988,
)

SEMITONE_TO_SGEN_MULT = (
    1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7,
    8, 8, 9, 9, 10, 11, 11, 12, 12, 13, 13, 14,
)

TRACKER_ROW_SGEN = (
    (262, "both", 4),
    (330, "both", 4),
    (392, "both", 4),
    (440, "both", 4),
    (523, "left", 4),
    (440, "right", 4),
    (392, "both", 4),
    (262, "both", 4),
)


def host_key_at_point(x, y):
    kx0, ky0, kw, kh = 66, 1024, 41, 112
    if not (ky0 <= y <= ky0 + kh and kx0 <= x < kx0 + kw * 14):
        return None
    if y < ky0 + 68:
        black_keys = (
            (0, 1), (1, 3), (3, 6), (4, 8), (5, 10),
            (7, 13), (8, 15), (10, 18), (11, 20), (12, 22),
        )
        for after_w, semi in black_keys:
            bx = kx0 + (after_w + 1) * kw - 13
            if bx <= x < bx + 24:
                return semi
    white_semis = (0, 2, 4, 5, 7, 9, 11, 12, 14, 16, 17, 19, 21, 23)
    idx = (x - kx0) // kw
    if 0 <= idx < len(white_semis):
        return white_semis[idx]
    return None


def host_row_at_point(x, y):
    if 52 <= x <= 668 and 760 <= y < 760 + 8 * 25:
        return (y - 760) // 25
    return None


def sleep_ms(ms):
    if ms > 0:
        select.poll().poll(int(ms))


class DebugUsb(jam.JamUsb):
    ALLOWED = jam.JamUsb.ALLOWED + (
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
            semi = host_key_at_point(x, y)
            row = host_row_at_point(x, y)
            if semi is not None:
                hz = SEMITONE_HZ[semi]
                idx = jam.encode_note_index(gain_db=4, spk="both", duration_ms=160)
                atrk = jam.trigger_and_wait(usb, 0x0090 + semi, idx, timeout_ms=3500)
                adbg = decode_adbg(usb.read(0xc0, 0xba, 0, 0, 64))
                hit_kind = "piano_key"
                hit_index = semi
                QWAVE_STATE["last_touch_x"] = x
                QWAVE_STATE["last_touch_y"] = y
                QWAVE_STATE["last_hit"] = f"piano_key:{semi}({hz}Hz)"
                return {
                    "ok": atrk["last_outcome"] == 1,
                    "x": x,
                    "y": y,
                    "hit_kind": hit_kind,
                    "hit_index": hit_index,
                    "sgen_mult": SEMITONE_TO_SGEN_MULT[semi],
                    "freq_hz": hz,
                    "gain_db": 4,
                    "qwave": dict(QWAVE_STATE),
                    "tracker": atrk,
                    "adbg": adbg,
                }
            if row is not None:
                hz, spk, gain_db = TRACKER_ROW_SGEN[row % 8]
                idx = jam.encode_note_index(gain_db=gain_db, spk=spk, duration_ms=160)
                atrk = jam.trigger_and_wait(usb, 0x0081 + (row % 8), idx, timeout_ms=3500)
                atrk["active_row"] = row
                adbg = decode_adbg(usb.read(0xc0, 0xba, 0, 0, 64))
                hit_kind = "tracker_row"
                hit_index = row
                QWAVE_STATE["last_touch_x"] = x
                QWAVE_STATE["last_touch_y"] = y
                QWAVE_STATE["last_hit"] = f"tracker_row:{row}({hz}Hz)"
                return {
                    "ok": atrk["last_outcome"] == 1,
                    "x": x,
                    "y": y,
                    "hit_kind": hit_kind,
                    "hit_index": hit_index,
                    "sgen_mult": 16,
                    "freq_hz": hz,
                    "gain_db": gain_db,
                    "qwave": dict(QWAVE_STATE),
                    "tracker": atrk,
                    "adbg": adbg,
                }
            usb.read(0xc0, 0xb9, 0x0000, y, 64)
            res = exec_debug_op(usb, (0 << 8) | 9, x, timeout_ms=3500, initial_wait_ms=420)
            atrk = jam.decode_atrk(usb.read(0xc0, 0xb8, 0, 0, 64))
            hit_code = res["result"] & 0xFFFF
            hit_kind = "none"
            hit_index = None
            if (hit_code & 0xFF00) == 0x0300:
                hit_kind = "qwave_tab"
                hit_index = hit_code & 0xFF
                QWAVE_STATE["active_tab"] = hit_index
                QWAVE_STATE["scroll_y"] = 0
            elif (hit_code & 0xFF00) == 0x0400:
                hit_kind = "qwave_scroll"
                hit_index = hit_code & 0xFF
                QWAVE_STATE["scroll_y"] = hit_index
            elif 1176 <= y <= 1198:
                for tab_i in range(3):
                    tx = 60 + tab_i * 180
                    if tx <= x < tx + 172:
                        hit_kind = "qwave_tab"
                        hit_index = tab_i
                        QWAVE_STATE["active_tab"] = tab_i
                        QWAVE_STATE["scroll_y"] = 0
            elif 54 <= x <= 666 and 1200 <= y <= 1326:
                hit_kind = "qwave_scroll"
                delta = -1 if y < 1262 else 1
                QWAVE_STATE["scroll_y"] = max(0, min(7, QWAVE_STATE["scroll_y"] + delta))
                hit_index = QWAVE_STATE["scroll_y"]
            QWAVE_STATE["last_touch_x"] = x
            QWAVE_STATE["last_touch_y"] = y
            QWAVE_STATE["last_hit"] = f"{hit_kind}:{hit_index}"
            return {
                "ok": res["status"] == 1,
                "x": x,
                "y": y,
                "hit_kind": hit_kind,
                "hit_index": hit_index,
                "qwave": dict(QWAVE_STATE),
                "tracker": atrk,
                "adbg": res,
            }

        if cmd == "scroll":
            delta = int(str(req.get("delta", 1)), 0)
            tab_arg = req.get("tab", None)
            if tab_arg is not None:
                tab_val = int(str(tab_arg), 0) % 3
                QWAVE_STATE["active_tab"] = tab_val
            else:
                tab_val = 0xFFFF
            QWAVE_STATE["scroll_y"] = max(0, min(7, QWAVE_STATE["scroll_y"] + delta))
            usb.read(0xc0, 0xb9, 0x0000, tab_val & 0xFFFF, 64)
            res = exec_debug_op(usb, (4 << 8) | 9, delta & 0xFFFF, timeout_ms=2500, initial_wait_ms=60)
            return {
                "ok": True,
                "delta": delta,
                "qwave": dict(QWAVE_STATE),
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

        if cmd == "gc7272":
            action = int(str(req.get("action", 5)), 0) & 0xFFFF
            res = exec_debug_op(usb, (5 << 8) | 9, action, timeout_ms=4000, initial_wait_ms=120)
            return {
                "ok": res["status"] == 1,
                "action": action,
                "result_hex": f"0x{res['result']:08x}",
                "buf_hex": res["buf_hex"],
                "buf_u32_hex": res["buf_u32_hex"],
                "adbg": res,
            }

        if cmd == "audio_scope":
            reg = int(str(req.get("reg", "0x22a4")), 0) & 0xFFFF
            ms = max(5, min(250, int(str(req.get("ms", 40)), 0)))
            usb.read(0xc0, 0xb9, 0x0000, ms, 64)
            res = exec_debug_op(usb, (6 << 8) | 9, reg, timeout_ms=4000, initial_wait_ms=ms + 40)
            raw_bytes = bytes.fromhex(res["buf_hex"])
            s8 = lambda b: b - 256 if b >= 128 else b
            n_samp = raw_bytes[4] | (raw_bytes[5] << 8)
            return {
                "ok": res["status"] == 1,
                "reg": f"0x{reg:04x}",
                "window_ms": ms,
                "hz_left": res["result"] & 0xFFFF,
                "hz_right": (res["result"] >> 16) & 0xFFFF,
                "min_left": s8(raw_bytes[0]),
                "max_left": s8(raw_bytes[1]),
                "min_right": s8(raw_bytes[2]),
                "max_right": s8(raw_bytes[3]),
                "n_samples": n_samp,
                "zero_crossings_left": raw_bytes[6],
                "zero_crossings_right": raw_bytes[7],
                "waveform_left": [s8(b) for b in raw_bytes[8:32]],
                "adbg": res,
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
    if cmd == "scroll":
        req_out = {"cmd": "scroll", "delta": parts[1] if len(parts) > 1 else "1"}
        if len(parts) > 2:
            req_out["tab"] = parts[2]
        return req_out
    if cmd == "jam":
        return {
            "cmd": "jam",
            "val": parts[1] if len(parts) > 1 else "0x0080",
            "idx": parts[2] if len(parts) > 2 else "0",
        }
    if cmd in ("touch_probe", "fdt_touch_info"):
        return {"cmd": "touch_probe"}
    if cmd == "gc7272":
        return {"cmd": "gc7272", "action": parts[1] if len(parts) > 1 else "5"}
    if cmd == "ota_reboot":
        return {"cmd": "ota_reboot"}
    return {"cmd": cmd}


QWAVE_STUDIO_HTML = """<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>AyeOS 5.6 x Qwave Live Touch &amp; Scroll Studio (BLU G75)</title>
<style>
  * { box-sizing: border-box; user-select: none; }
  body {
    margin: 0; padding: 16px;
    background: #05060a; color: #f4f7ff;
    font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
    display: flex; flex-direction: row; gap: 20px; align-items: flex-start;
  }
  .phone-shell {
    width: 396px; border: 3px solid #62e6c9; border-radius: 24px;
    background: #090c14; padding: 14px;
    box-shadow: 0 0 24px #0b2924;
  }
  .phone-header {
    display: flex; justify-content: space-between; align-items: center;
    padding-bottom: 8px; border-bottom: 1px solid #1f293d; margin-bottom: 10px;
  }
  .title-mint { color: #62e6c9; font-weight: bold; font-size: 16px; }
  .title-gold { color: #e8b05c; font-size: 12px; }
  .section-box {
    border: 1px solid #1f293d; border-radius: 10px; padding: 8px;
    margin-bottom: 10px; background: #05060a;
  }
  .sec-label {
    display: flex; justify-content: space-between;
    font-size: 11px; color: #e8b05c; margin-bottom: 6px;
  }
  .tracker-row {
    padding: 4px 6px; font-size: 12px; cursor: pointer;
    border-radius: 4px; margin-bottom: 2px; color: #d8e2ff;
    display: flex; justify-content: space-between;
  }
  .tracker-row:hover { background: #132238; }
  .tracker-row.active { background: #1a3330; color: #e8b05c; border: 1px solid #62e6c9; }
  .piano-wrap {
    position: relative; width: 350px; height: 112px;
    background: #101726; border: 2px solid #1f293d; border-radius: 6px;
    overflow: hidden; margin: 0 auto;
  }
  .white-keys {
    display: flex; width: 100%; height: 100%;
  }
  .wkey {
    flex: 1; border-right: 1px solid #05060a; background: #f4f7ff;
    color: #05060a; display: flex; align-items: flex-end; justify-content: center;
    padding-bottom: 8px; font-size: 11px; font-weight: bold; cursor: pointer;
  }
  .wkey:active, .wkey.active { background: #62e6c9; }
  .bkey {
    position: absolute; top: 0; width: 18px; height: 66px;
    background: #05060a; border: 1px solid #62e6c9; border-radius: 0 0 4px 4px;
    color: #e8b05c; font-size: 9px; display: flex; align-items: flex-end;
    justify-content: center; padding-bottom: 4px; cursor: pointer; z-index: 2;
  }
  .bkey:active, .bkey.active { background: #e8b05c; color: #05060a; }
  .qwave-tabs {
    display: flex; gap: 6px; margin-bottom: 8px;
  }
  .qtab {
    flex: 1; padding: 6px; text-align: center; font-size: 11px;
    background: #0d1322; color: #e8b05c; border: 1px solid #1f293d;
    border-radius: 6px; cursor: pointer; font-weight: bold;
  }
  .qtab.active { background: #62e6c9; color: #05060a; }
  .qwave-viewport {
    height: 170px; overflow-y: auto; border: 1px solid #1f293d;
    border-radius: 6px; padding: 8px; background: #070a12;
    scroll-behavior: smooth;
  }
  .qline {
    font-size: 12px; line-height: 22px; padding: 2px 4px;
    border-bottom: 1px dashed #111827;
  }
  .qline.head { color: #62e6c9; font-weight: bold; }
  .btn-bar {
    display: flex; gap: 8px; margin-top: 8px;
  }
  button {
    flex: 1; padding: 8px; background: #112222; color: #62e6c9;
    border: 1px solid #62e6c9; border-radius: 6px; cursor: pointer;
    font-family: inherit; font-size: 12px; font-weight: bold;
  }
  button:hover { background: #62e6c9; color: #05060a; }
  .side-panel {
    flex: 1; max-width: 540px;
  }
  pre {
    background: #090c14; border: 1px solid #1f293d; border-radius: 8px;
    padding: 10px; font-size: 12px; color: #62e6c9; overflow-x: auto;
  }
</style>
</head>
<body>
  <div class="phone-shell" id="phoneShell">
    <div class="phone-header">
      <div>
        <div class="title-mint">ayeOS 5_6 * BLU G75</div>
        <div class="title-gold">720x1640 TOUCH + QWAVE SCROLL</div>
      </div>
      <div id="statusBadge" class="title-mint">ONLINE</div>
    </div>

    <div class="section-box">
      <div class="sec-label">
        <span>03 / SCREAMTRACKER S3M (TAP ROW)</span>
        <span>MT6358 + AW87394</span>
      </div>
      <div id="trackerRows"></div>
      <div class="btn-bar">
        <button onclick="playFullSong()">PLAY 8-ROW S3M SONG</button>
      </div>
    </div>

    <div class="section-box">
      <div class="sec-label">
        <span>04 / 24-KEY PIANO (TOUCH OR KEYS Z..M)</span>
        <span id="lastNoteLabel">READY</span>
      </div>
      <div class="piano-wrap" id="pianoWrap">
        <div class="white-keys" id="whiteKeys"></div>
      </div>
    </div>

    <div class="section-box">
      <div class="sec-label">
        <span>05 / QWAVE SOVEREIGN BROWSER (SCROLL / TOUCH)</span>
        <span id="scrollPosLabel">SCROLL: 0</span>
      </div>
      <div class="qwave-tabs">
        <div class="qtab active" id="tab0" onclick="selectTab(0)">01: 8b.is</div>
        <div class="qtab" id="tab1" onclick="selectTab(1)">02: ESP32 Bus</div>
        <div class="qtab" id="tab2" onclick="selectTab(2)">03: GC7272</div>
      </div>
      <div class="qwave-viewport" id="qwaveViewport"></div>
      <div class="btn-bar">
        <button onclick="scrollQwave(-1)">SCROLL UP</button>
        <button onclick="scrollQwave(1)">SCROLL DOWN</button>
        <button onclick="sendTouch(600, 84, 'MODE TOGGLE (QWAVE/STUDIO)')">MODE: QWAVE/STUDIO</button>
      </div>
    </div>
  </div>

  <div class="side-panel">
    <div class="section-box">
      <div class="sec-label">
        <span>QWAVE x AYEOS LIVE HARDWARE TELEMETRY</span>
        <span>18d1:4ee0 (0562)</span>
      </div>
      <div style="font-size:13px; line-height:20px; color:#d8e2ff;">
        <p><b>Touch &amp; Scroll Active:</b> Click or drag any piano key, tracker row, or scroll the Qwave Browser viewport on the left (with mouse wheel, trackpad two-finger scroll, or touch). Every action sends exact integer coordinates `(x, y)` over `/tmp/ayeos-debug.sock` to the BLU G75!</p>
        <p><b>Physical Handset Controls:</b> Tap <b>Volume Up / Down</b> on the phone to scroll Qwave + step through ScreamTracker notes, or hold for 1 second to play the full 8-row S3M arpeggio!</p>
      </div>
      <pre id="telemetryBox">Connecting to /tmp/ayeos-debug.sock...</pre>
    </div>
  </div>

<script>
const ROWS = [
  {row:0, note:"C-4", hz:250, div:2, spk:"58+59 DUAL", y:770},
  {row:1, note:"G-4", hz:375, div:3, spk:"58+59 DUAL", y:795},
  {row:2, note:"C-5", hz:500, div:4, spk:"58+59 DUAL", y:820},
  {row:3, note:"E-5", hz:625, div:5, spk:"58+59 DUAL", y:845},
  {row:4, note:"G-5", hz:750, div:6, spk:"L:58 SOLO",  y:870},
  {row:5, note:"E-5", hz:625, div:5, spk:"R:59 SOLO",  y:895},
  {row:6, note:"C-5", hz:500, div:4, spk:"58+59 DUAL", y:920},
  {row:7, note:"C-4", hz:250, div:2, spk:"58+59 DUAL", y:945}
];

const WHITE_SEMI = [0,2,4,5,7,9,11,12,14,16,17,19,21,23];
const WHITE_NAMES = ["C4","D4","E4","F4","G4","A4","B4","C5","D5","E5","F5","G5","A5","B5"];
const BLACK_KEYS = [
  {after:0, semi:1, name:"C#"},
  {after:1, semi:3, name:"D#"},
  {after:3, semi:6, name:"F#"},
  {after:4, semi:8, name:"G#"},
  {after:5, semi:10, name:"A#"},
  {after:7, semi:13, name:"C#"},
  {after:8, semi:15, name:"D#"},
  {after:10, semi:18, name:"F#"},
  {after:11, semi:20, name:"G#"},
  {after:12, semi:22, name:"A#"}
];

const DOCS = [
  [
    "# qwave://8b.is/sanctuary (Zero-Float Mode)",
    "Welcome to AyeOS 5_6 + Qwave Sovereign Browser.",
    "Scroll with Qwave wheel/drag, touch UP/DN, or Vol!",
    "Every musical interval is p/q exact integer.",
    "MT6358 SGEN -> HPL/HPR -> Dual AW87394 SmartPAs.",
    "Tap any 24-key Piano note or S3M row above.",
    "Live USB EP0 socket: /tmp/ayeos-debug.sock",
    "Qwave Studio mirror: http://127.0.0.1:4380",
    "Constellation Ink: #05060a / #62e6c9 / #e8b05c",
    "Six Pillars: Compassion Understanding Moderation",
    "Love Respect Space * Fine touch from within.",
    "End of Tab 01 * {<3,<3,<3}+1 * LLL"
  ],
  [
    "# qwave://mothership/esp32-triad-flug",
    "BLU G75 Mothership <-> ESP32 Triad / FLUG Bus:",
    "- Node 01: ESP32-S3 USB-JTAG (303a:1001) ACTIVE",
    "- Node 02: Triad RF / WiFi-CSI / BLE Mesh Ready",
    "- Node 03: FLUG-OS Sensor & Synth Telemetry",
    "Guard: MediaTek BROM handshake never touches",
    "ESP32 nodes on the shared USB hub.",
    "ScreamTracker rows can cue ESP32 Triad triggers",
    "simultaneously with phone AW87394 speakers.",
    "Exact rational clock sync across mothership.",
    "Use ayeos-debug scroll or Qwave to inspect.",
    "End of Tab 02 * Mothership Bus Online."
  ],
  [
    "# qwave://hardware/gc7272-touch-spi0",
    "Panel: GalaxyCore GC7272 720x1640 @ 120Hz TDDI",
    "Bus: MediaTek SPI0 @ 0x1100a000 (9600 kHz)",
    "Pins: GPIO155=MI GPIO156=CSB GPIO157=MO 158=CLK",
    "Reset: GPIO23 (OUT=1) / IRQ: GPIO1 (IN Pull-Up)",
    "SPI0 RX IDM Status: 0x00000031 (ROM Bootloader)",
    "Note: GC7272 is a host-download TDDI chip",
    "(no internal flash; loads 64KB FW over SPI0",
    "from galaxycore-spi-gc7272.ko on boot).",
    "Touch works NOW via Qwave Browser (:4380),",
    "ayeos-debug touch/scroll, & Vol Up/Down keys!",
    "End of Tab 03 * Hardware Evidence Verified."
  ]
];

let currentTab = 0;
let currentScroll = 0;
let cmdQueue = [];
let runningCmd = false;

function buildTracker() {
  const box = document.getElementById("trackerRows");
  box.innerHTML = "";
  for (let i = 0; i < ROWS.length; i++) {
    const r = ROWS[i];
    const div = document.createElement("div");
    div.className = "tracker-row";
    div.id = "trow" + i;
    div.innerHTML = "<span>0" + i + "  " + r.note + "  " + r.hz + "Hz (D:" + r.div + ")</span><span>" + r.spk + "</span>";
    div.onclick = () => sendTouch(200, r.y, "ROW 0" + i + " (" + r.note + ")");
    box.appendChild(div);
  }
}

function buildPiano() {
  const wrap = document.getElementById("pianoWrap");
  const wbox = document.getElementById("whiteKeys");
  for (let i = 0; i < 14; i++) {
    const k = document.createElement("div");
    k.className = "wkey";
    k.textContent = WHITE_NAMES[i];
    const phoneX = 66 + i * 41 + 20;
    k.onclick = () => sendTouch(phoneX, 1110, WHITE_NAMES[i]);
    wbox.appendChild(k);
  }
  for (let i = 0; i < BLACK_KEYS.length; i++) {
    const bk = BLACK_KEYS[i];
    const el = document.createElement("div");
    el.className = "bkey";
    el.style.left = ((bk.after + 1) * 25 - 9) + "px";
    el.textContent = bk.name;
    const phoneX = 66 + (bk.after + 1) * 41 - 1;
    el.onclick = (ev) => { ev.stopPropagation(); sendTouch(phoneX, 1050, bk.name); };
    wrap.appendChild(el);
  }
}

function renderQwave() {
  for (let i = 0; i < 3; i++) {
    document.getElementById("tab" + i).className = (i === currentTab) ? "qtab active" : "qtab";
  }
  const vp = document.getElementById("qwaveViewport");
  vp.innerHTML = "";
  const lines = DOCS[currentTab];
  for (let i = 0; i < lines.length; i++) {
    const d = document.createElement("div");
    d.className = (i === 0) ? "qline head" : "qline";
    d.textContent = lines[i];
    vp.appendChild(d);
  }
  vp.scrollTop = currentScroll * 24;
  document.getElementById("scrollPosLabel").textContent = "TAB 0" + (currentTab + 1) + " / SCROLL: " + currentScroll;
}

function apiPost(payload, label) {
  if (label) {
    document.getElementById("lastNoteLabel").textContent = label;
  }
  cmdQueue.push(payload);
  drainQueue();
}

async function drainQueue() {
  if (runningCmd) return;
  runningCmd = true;
  while (cmdQueue.length > 0) {
    const payload = cmdQueue.shift();
    try {
      const resp = await fetch("/api/cmd", {
        method: "POST",
        headers: {"Content-Type": "application/json"},
        body: JSON.stringify(payload)
      });
      const data = await resp.json();
      document.getElementById("telemetryBox").textContent = JSON.stringify(data, null, 2);
      if (data.tracker && typeof data.tracker.active_row === "number") {
        for (let i = 0; i < 8; i++) {
          const el = document.getElementById("trow" + i);
          if (el) el.className = (i === data.tracker.active_row) ? "tracker-row active" : "tracker-row";
        }
      }
    } catch (e) {
      document.getElementById("telemetryBox").textContent = "Error: " + e;
    }
  }
  runningCmd = false;
}

function sendTouch(x, y, label) {
  apiPost({cmd: "touch", x: x, y: y}, label);
}

function playFullSong() {
  apiPost({cmd: "jam", val: "0x0080", idx: "0"}, "8-ROW S3M SONG");
}

function selectTab(tab) {
  currentTab = tab;
  currentScroll = 0;
  renderQwave();
  const tx = 100 + tab * 180;
  apiPost({cmd: "touch", x: tx, y: 1186}, "QWAVE TAB 0" + (tab + 1));
}

function scrollQwave(delta) {
  currentScroll = Math.max(0, Math.min(7, currentScroll + delta));
  renderQwave();
  apiPost({cmd: "scroll", delta: delta, tab: currentTab}, "QWAVE SCROLL " + currentScroll);
}

document.getElementById("qwaveViewport").addEventListener("wheel", (ev) => {
  ev.preventDefault();
  const delta = ev.deltaY > 0 ? 1 : -1;
  scrollQwave(delta);
}, {passive: false});

const KEYBOARD_SEMI = {
  "z":0, "s":1, "x":2, "d":3, "c":4, "v":5, "g":6, "b":7, "h":8, "n":9, "j":10, "m":11, ",":12
};
window.addEventListener("keydown", (ev) => {
  const k = ev.key.toLowerCase();
  if (k in KEYBOARD_SEMI) {
    const semi = KEYBOARD_SEMI[k];
    const wi = WHITE_SEMI.indexOf(semi);
    if (wi >= 0) {
      sendTouch(66 + wi * 41 + 20, 1110, WHITE_NAMES[wi]);
    }
  } else if (ev.key === "ArrowDown") {
    scrollQwave(1);
  } else if (ev.key === "ArrowUp") {
    scrollQwave(-1);
  }
});

buildTracker();
buildPiano();
renderQwave();
apiPost({cmd: "status"}, "READY");
</script>
</body>
</html>
"""


def http_request_complete(raw_bytes):
    if b"\r\n\r\n" not in raw_bytes:
        return False
    hdr_bytes, _, body_bytes = raw_bytes.partition(b"\r\n\r\n")
    hdr_text = hdr_bytes.decode("utf-8", errors="replace")
    content_len = 0
    for line in hdr_text.splitlines():
        if line.lower().startswith("content-length:"):
            try:
                content_len = int(line.split(":", 1)[1].strip(), 10)
            except ValueError:
                content_len = 0
    return len(body_bytes) >= content_len


def handle_http_request(raw_bytes, device_path=None):
    hdr_bytes, _, body_bytes = raw_bytes.partition(b"\r\n\r\n")
    header_part = hdr_bytes.decode("utf-8", errors="replace")
    body_part = body_bytes.decode("utf-8", errors="replace")
    first_line = header_part.splitlines()[0] if header_part else "GET / HTTP/1.1"
    parts = first_line.split()
    method = parts[0] if len(parts) > 0 else "GET"
    path = parts[1] if len(parts) > 1 else "/"

    if method == "POST" and path == "/api/cmd":
        try:
            req = json.loads(body_part.strip() or "{}")
            resp = execute_request(req, device_path=device_path)
        except Exception as exc:
            resp = {"ok": False, "error": str(exc)}
        payload = json.dumps(resp).encode("utf-8")
        headers = (
            "HTTP/1.1 200 OK\r\n"
            "Content-Type: application/json; charset=utf-8\r\n"
            f"Content-Length: {len(payload)}\r\n"
            "Connection: close\r\n\r\n"
        ).encode("utf-8")
        return headers + payload

    if path.startswith("/api/status"):
        try:
            resp = execute_request({"cmd": "status"}, device_path=device_path)
        except Exception as exc:
            resp = {"ok": False, "error": str(exc)}
        payload = json.dumps(resp).encode("utf-8")
        headers = (
            "HTTP/1.1 200 OK\r\n"
            "Content-Type: application/json; charset=utf-8\r\n"
            f"Content-Length: {len(payload)}\r\n"
            "Connection: close\r\n\r\n"
        ).encode("utf-8")
        return headers + payload

    payload = QWAVE_STUDIO_HTML.encode("utf-8")
    headers = (
        "HTTP/1.1 200 OK\r\n"
        "Content-Type: text/html; charset=utf-8\r\n"
        f"Content-Length: {len(payload)}\r\n"
        "Connection: close\r\n\r\n"
    ).encode("utf-8")
    return headers + payload


def serve_daemon(unix_path, tcp_host, tcp_port, http_port=DEFAULT_HTTP_PORT, device_path=None):
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

    http_srv = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    http_srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    http_srv.bind((tcp_host, http_port))
    http_srv.listen(8)
    http_srv.setblocking(False)

    poller = select.poll()
    poller.register(unix_srv.fileno(), select.POLLIN)
    poller.register(tcp_srv.fileno(), select.POLLIN)
    poller.register(http_srv.fileno(), select.POLLIN)
    clients = {}
    http_clients = {}
    buffers = {}

    print(
        json.dumps(
            {
                "daemon": "ayeos-debug-socket-v1",
                "unix_socket": unix_path,
                "tcp_socket": f"{tcp_host}:{tcp_port}",
                "qwave_http_studio": f"http://{tcp_host}:{http_port}",
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
                elif fd == http_srv.fileno():
                    conn, _ = http_srv.accept()
                    conn.setblocking(False)
                    http_clients[conn.fileno()] = conn
                    buffers[conn.fileno()] = b""
                    poller.register(conn.fileno(), select.POLLIN)
                elif fd in http_clients:
                    conn = http_clients[fd]
                    try:
                        chunk = conn.recv(8192)
                    except OSError:
                        chunk = b""
                    if chunk:
                        buffers[fd] += chunk
                    raw = buffers.get(fd, b"")
                    if http_request_complete(raw) or not chunk:
                        poller.unregister(fd)
                        http_clients.pop(fd, None)
                        buffers.pop(fd, None)
                        try:
                            out = handle_http_request(raw, device_path=device_path)
                            conn.setblocking(True)
                            conn.sendall(out)
                        except OSError:
                            pass
                        finally:
                            conn.close()
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
        for conn in http_clients.values():
            conn.close()
        unix_srv.close()
        tcp_srv.close()
        http_srv.close()
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
    parser.add_argument("--serve", action="store_true", help="Run background UNIX + TCP + Qwave HTTP Studio daemon")
    parser.add_argument("--unix-sock", default=DEFAULT_UNIX_SOCK, help="UNIX socket path")
    parser.add_argument("--tcp-host", default=DEFAULT_TCP_HOST, help="TCP bind host")
    parser.add_argument("--tcp-port", type=int, default=DEFAULT_TCP_PORT, help="TCP bind port")
    parser.add_argument("--http-port", type=int, default=DEFAULT_HTTP_PORT, help="Qwave HTTP Studio bind port")
    parser.add_argument("--device", help="Explicit /dev/bus/usb/BBB/DDD device path")
    parser.add_argument("--direct", action="store_true", help="Bypass socket daemon and talk to USB directly")
    parser.add_argument("command", nargs="*", help="Debug command (e.g. status, scroll 1 0, touch 160 1100, ota_reboot)")
    args = parser.parse_args()

    if args.serve:
        serve_daemon(args.unix_sock, args.tcp_host, args.tcp_port, http_port=args.http_port, device_path=args.device)
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
