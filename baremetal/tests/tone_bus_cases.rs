use super::*;
use audio_probe as a;
use pmic_probe as p;
use tone_bus::{Hardware, HardwareIo};
use usb_control::Io;

struct Mmio {
    regs: BTreeMap<usize, u32>,
    pm: BTreeMap<u16, u16>,
    pa: BTreeMap<(u8, u8), u8>,
    fifo: Vec<u8>,
    answer: u8,
    ops: usize,
    fail: usize,
    fault: bool,
    tick: u64,
    nack: bool,
    stall: bool,
    commands: Vec<u32>,
    writes_after_fault: usize,
}
impl Mmio {
    fn new() -> Self {
        Self {
            regs: [
                (a::PIN, 0x110),
                (a::BASE + 0x20, 0x10),
                (a::BASE + 0x34, 3),
                (p::BASE + 4, 1),
                (p::BASE + 0x9c, 1),
                (p::BASE + 0xa0, 1),
                (p::RDATA, p::INIT | p::SYNC),
            ]
            .into_iter()
            .collect(),
            pm: BTreeMap::new(),
            pa: BTreeMap::new(),
            fifo: vec![],
            answer: 0,
            ops: 0,
            fail: usize::MAX,
            fault: false,
            tick: 0,
            nack: false,
            stall: false,
            commands: vec![],
            writes_after_fault: 0,
        }
    }
    fn gate(&mut self) -> bool {
        if self.fault {
            return false;
        }
        self.ops += 1;
        if self.ops == self.fail {
            self.fault = true;
            return false;
        }
        true
    }
}
impl Io for Mmio {
    fn read8(&mut self, x: usize) -> u8 {
        if !self.gate() {
            return 0;
        }
        assert_eq!(x, a::BASE);
        self.regs.insert(a::BASE + 0xf4, 0);
        self.answer
    }
    fn read16(&mut self, x: usize) -> u16 {
        if !self.gate() {
            return 0;
        }
        *self.regs.get(&x).unwrap_or(&0) as u16
    }
    fn read32(&mut self, x: usize) -> u32 {
        if !self.gate() {
            return 0;
        }
        *self.regs.get(&x).unwrap_or(&0)
    }
    fn write8(&mut self, x: usize, v: u8) {
        if !self.gate() {
            return;
        }
        assert_eq!(x, a::BASE);
        self.fifo.push(v);
    }
    fn write16(&mut self, x: usize, v: u16) {
        if !self.gate() {
            return;
        }
        if x == a::BASE + 0xc {
            let old = *self.regs.get(&x).unwrap_or(&0);
            self.regs.insert(x, old & !u32::from(v));
        } else if x == a::BASE + 0x38 {
            self.fifo.clear();
            self.regs.insert(a::BASE + 0xf4, 0);
        } else if x == a::BASE + 0x50 {
            self.regs.insert(a::BASE + 0x24, 0);
            self.regs.insert(a::BASE + 0xc, 0);
        } else if x == a::BASE + 0x24 {
            assert_eq!(v, 1);
            let addr = (self.regs[&(a::BASE + 4)] >> 1) as u8;
            let reg = self.fifo[0];
            if self.regs[&(a::BASE + 0x18)] == 1 {
                assert_eq!(self.regs[&(a::BASE + 0x10)], 0xa8);
                assert_eq!(self.regs[&(a::BASE + 0x14)], 2);
                assert_eq!(self.fifo.len(), 2);
                if !self.nack {
                    self.pa.insert((addr, reg), self.fifo[1]);
                }
                self.regs.insert(a::BASE + 0xf4, 0);
            } else {
                assert_eq!(self.regs[&(a::BASE + 0x10)], 0xba);
                assert_eq!(self.fifo.len(), 1);
                self.answer = *self.pa.get(&(addr, reg)).unwrap_or(&0);
                self.regs.insert(a::BASE + 0xf4, 0x10);
            }
            self.regs.insert(
                a::BASE + 0xc,
                if self.stall {
                    0
                } else if self.nack {
                    2
                } else {
                    1
                },
            );
            self.regs.insert(x, u32::from(self.stall));
        } else {
            self.regs.insert(x, u32::from(v));
        }
    }
    fn write32(&mut self, x: usize, v: u32) {
        if !self.gate() {
            return;
        }
        if x == p::CMD {
            assert_eq!(
                self.regs[&p::RDATA] & (p::INIT | p::SYNC | 0xf0000),
                p::INIT | p::SYNC
            );
            self.commands.push(v);
            let reg = ((v >> 16) & 0x7fff) as u16 * 2;
            if v >> 31 != 0 {
                self.pm.insert(reg, v as u16);
            } else {
                self.regs.insert(
                    p::RDATA,
                    p::INIT | p::SYNC | (6 << 16) | u32::from(*self.pm.get(&reg).unwrap_or(&0)),
                );
            }
            if self.stall {
                self.regs.insert(p::RDATA, p::INIT | p::SYNC | (2 << 16));
            }
        } else {
            assert_eq!(x, p::CLEAR);
            assert_eq!(v, 1);
            self.regs.insert(p::RDATA, p::INIT | p::SYNC);
        }
    }
    fn faulted(&self) -> bool {
        self.fault
    }
    fn settle(&mut self) {
        panic!("no uncalibrated delay");
    }
}
impl HardwareIo for Mmio {
    fn counter(&mut self) -> Result<u64> {
        self.tick += 13;
        Ok(self.tick)
    }
    fn frequency(&mut self) -> u64 {
        tone::HZ
    }
    fn fresh(&mut self) {
        self.fault = false;
    }
    fn armed(&mut self, _: bool, _: [u16; 17]) {}
}

fn transaction(b: &mut Hardware<Mmio>, kind: u8) -> Result<()> {
    match kind {
        0 => b.amp_write(0x58, 2, 0x3f),
        1 => b.amp_read(0x59, 0).map(|_| ()),
        2 => b.pmic_write(0x22c2, 1),
        _ => b.pmic_read(0x40e).map(|_| ()),
    }
}
#[test]
fn fifo_write_and_pwrap_write_encode_and_read_back_actual_registers() {
    let mut b = Hardware::prepare(Mmio::new()).unwrap();
    b.amp_write(0x59, 4, 0x2b).unwrap();
    assert_eq!(b.amp_read(0x59, 4), Ok(0x2b));
    b.pmic_write(0x22c2, 1).unwrap();
    assert_eq!(b.pmic_read(0x22c2), Ok(1));
    assert_eq!(b.io.commands[0], 0x9161_0001);
    assert_eq!(b.io.commands[1], 0x1161_0000);
    b.restore().unwrap();
    assert_eq!(b.io.regs[&(a::BASE + 0x20)], 0x10);
    assert_eq!(b.io.regs[&p::RDATA], p::INIT | p::SYNC);
}
#[test]
fn every_transaction_mmio_fault_returns_error_and_allows_independent_cleanup() {
    for kind in 0..4 {
        let mut baseline = Hardware::prepare(Mmio::new()).unwrap();
        let before = baseline.io.ops;
        transaction(&mut baseline, kind).unwrap();
        for fail in before + 1..=baseline.io.ops {
            let mut b = Hardware::prepare(Mmio::new()).unwrap();
            b.io.fail = fail;
            assert!(transaction(&mut b, kind).is_err(), "kind={kind} at={fail}");
            assert!(b.io.fault);
            b.recover(kind >= 2).unwrap();
            b.restore().unwrap();
            b.amp_write(0x58, 2, 0).unwrap();
            b.amp_write(0x59, 2, 0).unwrap();
            b.pmic_write(0x229a, 0).unwrap();
        }
    }
}
#[test]
fn inherited_pending_pwrap_never_consumed_and_clock_gate_never_enabled() {
    for value in [p::INIT | p::SYNC | (6 << 16), p::INIT | p::SYNC | (2 << 16)] {
        let mut m = Mmio::new();
        m.regs.insert(p::RDATA, value);
        assert!(matches!(Hardware::prepare(m), Err(Error::Busy)));
    }
    let mut m = Mmio::new();
    m.regs.insert(a::INFRA, 0x80);
    assert!(matches!(Hardware::prepare(m), Err(Error::Gate)));
}
#[test]
fn nack_timeout_and_owned_late_response_have_bounded_recovery() {
    let mut b = Hardware::prepare(Mmio::new()).unwrap();
    b.io.nack = true;
    assert_eq!(b.amp_write(0x58, 2, 0), Err(Error::Nack));
    b.io.nack = false;
    b.recover(false).unwrap();
    b.io.stall = true;
    assert_eq!(b.amp_write(0x58, 2, 0), Err(Error::Timeout));
    b.io.stall = false;
    b.recover(false).unwrap();
    b.io.stall = true;
    assert_eq!(b.pmic_read(0x40e), Err(Error::Timeout));
    b.io.regs
        .insert(p::RDATA, p::INIT | p::SYNC | (6 << 16) | 0x5aa5);
    b.io.stall = false;
    b.recover(true).unwrap();
    assert_eq!(b.io.regs[&p::RDATA], p::INIT | p::SYNC);
}
