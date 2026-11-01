#![allow(dead_code)]
#[path = "../src/audio_board.rs"]
mod audio_board;
#[path = "../src/audio_probe.rs"]
mod audio_probe;
#[path = "../src/board_pages.rs"]
mod board_pages;
#[path = "../src/board_screen.rs"]
mod board_screen;
#[path = "../src/device_tree.rs"]
mod device_tree;
#[path = "../src/pmic_probe.rs"]
mod pmic_probe;
#[path = "../src/tone.rs"]
mod tone;
#[path = "../src/tone_board.rs"]
mod tone_board;
mod tone_board_cases;
#[path = "../src/tone_bus.rs"]
mod tone_bus;
mod tone_bus_cases;
#[path = "../src/usb_control.rs"]
mod usb_control;
use std::collections::BTreeMap;
use tone::{Bus, Error, Result};

struct Model {
    pm: BTreeMap<u16, u16>,
    pa: BTreeMap<(u8, u8), u8>,
    ops: usize,
    fail: usize,
    after: bool,
    tick: u64,
    step: u64,
    hz: u64,
    armed: bool,
    ever_armed: bool,
    cleanup: bool,
    off_attempts: [bool; 2],
    writes: Vec<(u16, u16)>,
    freeze: bool,
    broken: u8,
    restore_bad: bool,
    corrupt: Option<u16>,
    standby_3e: bool,
}
impl Model {
    fn new() -> Self {
        Self {
            pm: [
                (0x40e, 0x5aa5),
                (0xa, 0x5820),
                (0x1ac4, 1),
                (0x1ad2, 0x8000),
            ]
            .into_iter()
            .collect(),
            pa: [
                ((0x58, 0), 0x23),
                ((0x59, 0), 0x23),
                ((0x58, 2), 0x3e),
                ((0x59, 2), 0x3e),
            ]
            .into_iter()
            .collect(),
            ops: 0,
            fail: usize::MAX,
            after: false,
            tick: 1,
            step: 1300,
            hz: tone::HZ,
            armed: false,
            ever_armed: false,
            cleanup: false,
            off_attempts: [false; 2],
            writes: vec![],
            freeze: false,
            broken: 0,
            restore_bad: false,
            corrupt: None,
            standby_3e: false,
        }
    }
    fn op(&mut self) -> Result<()> {
        self.ops += 1;
        if self.ops == self.fail {
            Err(Error::Nack)
        } else {
            Ok(())
        }
    }
}
impl Bus for Model {
    fn ticks(&mut self) -> Result<u64> {
        if !(self.freeze && self.ever_armed) {
            self.tick += self.step;
        }
        Ok(self.tick)
    }
    fn frequency(&mut self) -> u64 {
        self.hz
    }
    fn arm(&mut self, a: bool) {
        self.armed = a;
        self.ever_armed |= a;
    }
    fn pmic_read(&mut self, r: u16) -> Result<u16> {
        self.op()?;
        if self.cleanup && self.broken == 1 {
            return Err(Error::Fault);
        }
        Ok(*self.pm.get(&r).unwrap_or(&0) ^ u16::from(self.corrupt == Some(r)))
    }
    fn pmic_write(&mut self, r: u16, v: u16) -> Result<()> {
        let result = self.op();
        if result.is_err() && !self.after {
            return result;
        }
        assert!(
            ![0x1ac4, 0x1ad2].contains(&r),
            "always-on supply is never owned"
        );
        if r == 0x22c0 && v & 0xc0 == 0x80 {
            assert_eq!(self.pm[&0x248a] & 0xf9f, 0xf9f, "unmute only at -40dB");
            assert_eq!(self.pm[&0x2488], if self.standby_3e { 0x0b } else { 0x10b });
            assert_eq!(self.pm[&0x2416] & 0xf, 7);
            assert_eq!(self.pm[&0x22c2], 1, "documented CFG1, no guessed channels");
            for a in [0x58, 0x59] {
                for (reg, x) in tone::MUSIC {
                    assert_eq!(
                        self.pa[&(a, reg)],
                        if a == 0x59 && reg == 4 { 0x2b } else { x }
                    );
                }
            }
        }
        let v = if self.standby_3e && r == 0x2488 {
            v & 0x7f
        } else {
            v
        };
        self.writes.push((r, v));
        self.pm.insert(r, v);
        result
    }
    fn amp_read(&mut self, a: u8, r: u8) -> Result<u8> {
        self.op()?;
        if self.cleanup && self.broken == a {
            return Err(Error::Nack);
        }
        Ok(*self.pa.get(&(a, r)).unwrap_or(&0))
    }
    fn amp_write(&mut self, a: u8, r: u8, v: u8) -> Result<()> {
        if self.cleanup && r == 2 && v == 0 {
            self.off_attempts[(a - 0x58) as usize] = true;
        }
        let result = self.op();
        if result.is_err() && !self.after {
            return result;
        }
        if self.cleanup && self.broken == a {
            return Err(Error::Nack);
        }
        if r == 0 {
            assert_eq!(v, 0xaa);
            if self.standby_3e {
                self.pa.insert((a, 2), 0x3e);
            }
        } else if r == 2 && v == 0 && self.standby_3e {
            self.pa.insert((a, 2), 0x3e);
        } else {
            self.pa.insert((a, r), v);
        }
        result
    }
    fn recover(&mut self, _: bool) -> Result<()> {
        self.cleanup = true;
        Ok(())
    }
    fn restore(&mut self) -> Result<()> {
        if self.restore_bad {
            Err(Error::Restore)
        } else {
            Ok(())
        }
    }
}

#[test]
fn complete_tone_is_attenuated_ordered_bounded_and_exportable() {
    let mut m = Model::new();
    let r = tone::run(&mut m, true);
    assert_eq!(r.outcome, 1);
    assert!(r.verified_off() && r.restored);
    assert_eq!(m.off_attempts, [true; 2]);
    assert!(!m.armed);
    assert!(r.closed - r.opened >= tone::TONE_US * 13);
    assert!(r.closed - r.opened < (tone::TONE_US + 20_000) * 13);
    assert_eq!(m.pa[&(0x58, 2)], 0);
    assert_eq!(m.pa[&(0x59, 2)], 0);
    let mut m3e = Model::new();
    m3e.standby_3e = true;
    let r3e = tone::run(&mut m3e, true);
    assert_eq!(r3e.outcome, 1);
    assert!(r3e.verified_off() && r3e.restored);
    assert_eq!(m3e.pa[&(0x58, 2)], 0x3e);
    assert_eq!(m3e.pa[&(0x59, 2)], 0x3e);
    let encoded = r.encode("0123456789abcdef0123456789abcdef01234567", false, 1, 2, 3);
    assert_eq!(encoded.len(), 6400);
    if let Ok(path) = std::env::var("AYEOS_TONE_TEST_DUMP") {
        use std::io::Write;
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .unwrap()
            .write_all(&encoded)
            .unwrap();
    }
}
#[test]
fn every_logical_bus_failure_before_or_after_effect_is_retained_and_cleans_up() {
    let mut baseline = Model::new();
    tone::run(&mut baseline, true);
    for after in [false, true] {
        for fail in 1..=baseline.ops {
            let mut m = Model::new();
            m.fail = fail;
            m.after = after;
            let r = tone::run(&mut m, true);
            assert_ne!(r.outcome, 1, "operation {fail} after={after}");
            if m.ever_armed {
                assert_eq!(m.off_attempts, [true; 2], "operation {fail}");
                assert!(!m.armed);
            } else {
                assert!(m.writes.is_empty());
            }
        }
    }
}
#[test]
fn dead_bus_cannot_skip_other_bus_or_manufacture_verified_off() {
    for broken in [1, 0x58, 0x59] {
        let mut m = Model::new();
        m.broken = broken;
        let r = tone::run(&mut m, true);
        assert_eq!(r.outcome, 4);
        assert!(!r.verified_off());
        assert_eq!(m.off_attempts, [true; 2]);
        if broken != 1 {
            assert_eq!(m.pm[&0x2416] & 0xf, 0);
        }
        if broken != 0x58 {
            assert_eq!(m.pa[&(0x58, 2)], 0);
        }
        if broken != 0x59 {
            assert_eq!(m.pa[&(0x59, 2)], 0);
        }
    }
}
#[test]
fn gates_identity_supply_and_timer_reject_without_value_writes() {
    for which in 0..8 {
        let mut m = Model::new();
        match which {
            1 => m.hz = 0,
            2 => m.step = 0,
            3 => {
                m.pm.insert(0x1ac4, 0);
            }
            4 => {
                m.pm.insert(0x1ad2, 0);
            }
            5 => {
                m.pa.insert((0x59, 0), 0x39);
            }
            6 => {
                m.pm.insert(0x2408, 6);
            }
            7 => {
                m.pm.insert(0xa, 0x1234);
            }
            _ => {}
        }
        let r = tone::run(&mut m, which != 0);
        assert_eq!(r.outcome, 2);
        assert!(!m.ever_armed && m.writes.is_empty());
        assert!(!r.verified_off());
    }
}
#[test]
fn stopped_counter_after_arm_still_attempts_and_verifies_shutdown() {
    let mut m = Model::new();
    m.freeze = true;
    let r = tone::run(&mut m, true);
    assert_eq!(r.error, Error::Timer as u32);
    assert!(r.verified_off());
    assert_eq!(m.off_attempts, [true; 2]);
    assert_ne!(r.outcome, 1);
}
#[test]
fn restore_failure_is_not_complete_even_if_audio_off() {
    let mut m = Model::new();
    m.restore_bad = true;
    let r = tone::run(&mut m, true);
    assert!(r.verified_off());
    assert_eq!(r.outcome, 3);
    assert!(!r.restored);
}
#[test]
fn mismatching_configuration_readback_aborts_before_unmute() {
    let mut m = Model::new();
    m.corrupt = Some(0x22c2);
    let r = tone::run(&mut m, true);
    assert_eq!(r.error, Error::Readback as u32);
    assert_eq!(r.opened, 0);
    assert!(r.verified_off());
    assert_eq!(r.outcome, 3);
}
#[test]
fn expired_startup_deadline_never_opens_the_source() {
    let mut m = Model::new();
    m.step = tone::HZ;
    let r = tone::run(&mut m, true);
    assert_eq!(r.error, Error::Deadline as u32);
    assert_eq!(r.opened, 0);
    assert!(m.writes.is_empty());
}
#[test]
fn usb_tone_snapshot_is_read_only_and_survives_reset() {
    use usb_control::{Control, Reply};
    let mut m = Model::new();
    let r = tone::run(&mut m, true);
    let bytes = r.encode("0123456789abcdef0123456789abcdef01234567", false, 1, 2, 3);
    let mut c = Control::new(b"fdt");
    c.set_tone(&bytes);
    let mut out = [0; 64];
    let info = [0xc0, 0xb6, 0, 0, 0, 0, 64, 0];
    assert_eq!(c.setup(info, &mut out), Reply::Stall);
    c.configuration = 1;
    assert_eq!(c.setup(info, &mut out), Reply::Data(64));
    assert_eq!(&out[..4], b"ATIF");
    let before = out;
    c.reset();
    c.configuration = 1;
    assert_eq!(c.setup(info, &mut out), Reply::Data(64));
    assert_eq!(out, before);
    assert_eq!(
        c.setup([0x40, 0xb7, 0, 0, 0, 0, 64, 0], &mut out),
        Reply::Stall
    );
    let mut got = vec![];
    for n in 0..134u16 {
        let [lo, hi] = n.to_le_bytes();
        assert_eq!(
            c.setup([0xc0, 0xb7, lo, hi, 0, 0, 64, 0], &mut out),
            Reply::Data(64)
        );
        let len = u16::from_le_bytes(out[8..10].try_into().unwrap()) as usize;
        assert_eq!(&out[..4], b"ATCH");
        assert_eq!(
            board_pages::crc32(&out[16..16 + len]),
            u32::from_le_bytes(out[12..16].try_into().unwrap())
        );
        got.extend_from_slice(&out[16..16 + len]);
    }
    assert_eq!(got, bytes);
    assert_eq!(
        c.setup([0xc0, 0xb7, 134, 0, 0, 0, 64, 0], &mut out),
        Reply::Stall
    );
    assert_eq!(usb_control::DEVICE[12..14], [0x62, 5]);
}
