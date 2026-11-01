// SPDX-License-Identifier: GPL-2.0-only
// MT6768 FIFO/WACS2 protocols: MediaTek i2c-mt65xx.c / mtk-pmic-wrap.c.
// Rust adaptation OpenAI GPT-6 Astra, 2026. No DMA, clock retuning or bus scans.
use crate::{
    audio_probe as i2c, pmic_probe as pm,
    tone::{self, Error, Result},
    usb_control::Io,
};

pub trait HardwareIo: Io {
    fn counter(&mut self) -> Result<u64>;
    fn frequency(&mut self) -> u64;
    /// Used ONLY at explicit cleanup boundaries; never continues a failed setup.
    fn fresh(&mut self);
    fn armed(&mut self, armed: bool, saved: [u16; 17]);
}
pub struct Hardware<I> {
    pub io: I,
    saved: [u16; 17],
    available: bool,
    pa_owned: bool,
    pm_owned: bool,
}
impl<I: HardwareIo> Hardware<I> {
    pub fn prepare(mut io: I) -> Result<Self> {
        if io.read32(i2c::TOP) & 0x80 != 0
            || io.read32(i2c::INFRA) & 0x80 != 0
            || (io.read32(i2c::PIN) >> 4) & 0xff != 0x11
        {
            return Err(Error::Gate);
        }
        if io.read16(i2c::BASE + 0x24) & 1 != 0 || (io.read16(i2c::BASE + 0xf4) >> 4) & 15 != 0 {
            return Err(Error::Busy);
        }
        let mut saved = [0; 17];
        for (v, off) in saved.iter_mut().zip(i2c::OFFSETS) {
            *v = io.read16(i2c::BASE + off);
        }
        if saved[6] == 0 || saved[9] & 1 != 0 || saved[10] & 3 != 3 {
            return Err(Error::Gate);
        }
        if io.read32(pm::INFRA) & 2 != 0
            || io.read32(pm::BASE) != 0
            || io.read32(pm::BASE + 4) != 1
            || io.read32(pm::BASE + 0x7c) != 0
            || io.read32(pm::BASE + 0x9c) != 1
            || io.read32(pm::BASE + 0xa0) != 1
        {
            return Err(Error::Gate);
        }
        let v = io.read32(pm::RDATA);
        if io.faulted() {
            return Err(Error::Fault);
        }
        if v & pm::INIT == 0 || (v >> 16) & 7 != 0 || v & (1 << 19) != 0 {
            return Err(Error::Busy);
        }
        Ok(Self {
            io,
            saved,
            available: true,
            pa_owned: false,
            pm_owned: false,
        })
    }
    /// Only after the single boot CPU has armed ownership. A response pending at
    /// a fatal exit then belongs to this path. No inherited FSM6 is consumed.
    pub fn emergency(io: I, saved: [u16; 17]) -> Self {
        Self {
            io,
            saved,
            available: true,
            pa_owned: true,
            pm_owned: true,
        }
    }
    fn fault(&self) -> Result<()> {
        if self.io.faulted() {
            Err(Error::Fault)
        } else {
            Ok(())
        }
    }
    fn elapsed(&mut self, start: u64) -> Result<()> {
        let now = self.io.counter()?;
        if now < start {
            return Err(Error::Timer);
        }
        if now - start >= tone::HZ / 50 {
            return Err(Error::Timeout);
        } // 20ms per phase
        Ok(())
    }
    fn wait_pm(&mut self, response: bool, recover: bool) -> Result<u32> {
        // During cleanup the work cap is authoritative even if the counter fails.
        let start = self.io.counter().ok();
        for _ in 0..pm::BUDGET {
            let v = self.io.read32(pm::RDATA);
            self.fault()?;
            if v & pm::INIT == 0 {
                return Err(Error::Gate);
            }
            let fsm = (v >> 16) & 7;
            if response && fsm == 6 {
                return Ok(v);
            }
            if !response && fsm == 0 && v & pm::SYNC != 0 && v & (1 << 19) == 0 {
                return Ok(v);
            }
            if recover && fsm == 6 && self.pm_owned {
                self.io.write32(pm::CLEAR, 1);
                self.fault()?;
            }
            if let Some(start) = start {
                self.elapsed(start)?;
            }
        }
        Err(Error::Timeout)
    }
    fn ready_pm(&mut self) -> Result<()> {
        let v = self.io.read32(pm::RDATA);
        self.fault()?;
        if (v >> 16) & 7 != 0 || v & (1 << 19) != 0 {
            return Err(Error::Busy);
        }
        self.wait_pm(false, false).map(|_| ())
    }
    fn transfer(&mut self, addr: u8, reg: u8, value: Option<u8>) -> Result<u8> {
        if ![0x58, 0x59].contains(&addr) || reg > 0x7f {
            return Err(Error::Gate);
        }
        self.fault()?;
        if self.io.read16(i2c::BASE + 0x24) & 1 != 0 {
            return Err(Error::Busy);
        }
        self.pa_owned = true; // before any possible faulting configuration write
        self.io.write16(i2c::BASE + 8, 0);
        self.io
            .write16(i2c::BASE + 0x10, if value.is_some() { 0xa8 } else { 0xba });
        self.io.write16(i2c::BASE + 4, u16::from(addr) << 1);
        let irq = self.io.read16(i2c::BASE + 0xc);
        if irq != 0 {
            self.io.write16(i2c::BASE + 0xc, irq);
        }
        self.io.write16(i2c::BASE + 0x38, 1);
        self.io
            .write16(i2c::BASE + 0x14, if value.is_some() { 2 } else { 1 });
        self.io
            .write16(i2c::BASE + 0x44, if value.is_some() { 0 } else { 1 });
        self.io
            .write16(i2c::BASE + 0x18, if value.is_some() { 1 } else { 2 });
        self.io.write8(i2c::BASE, reg);
        if let Some(v) = value {
            self.io.write8(i2c::BASE, v);
        }
        self.io.write16(i2c::BASE + 0x24, 1);
        self.fault()?;
        let start = self.io.counter().ok();
        for _ in 0..i2c::BUDGET {
            let irq = self.io.read16(i2c::BASE + 0xc);
            self.fault()?;
            if irq != 0 {
                self.io.write16(i2c::BASE + 0xc, irq);
            }
            self.fault()?;
            if irq & 0x208 != 0 {
                return Err(Error::Controller);
            }
            if irq & 6 != 0 {
                return Err(Error::Nack);
            }
            if irq & 1 != 0 {
                let n = (self.io.read16(i2c::BASE + 0xf4) >> 4) & 15;
                self.fault()?;
                if n != u16::from(value.is_none()) {
                    return Err(Error::Fifo);
                }
                let answer = if value.is_none() {
                    self.io.read8(i2c::BASE)
                } else {
                    0
                };
                self.fault()?;
                self.wait_pa()?;
                self.pa_owned = false;
                return Ok(answer);
            }
            if let Some(start) = start {
                self.elapsed(start)?;
            }
        }
        Err(Error::Timeout)
    }
    fn wait_pa(&mut self) -> Result<()> {
        for _ in 0..256 {
            let idle = self.io.read16(i2c::BASE + 0x24) & 1 == 0;
            self.fault()?;
            if idle {
                return Ok(());
            }
        }
        Err(Error::Timeout)
    }
    fn restore_pa(&mut self) -> Result<()> {
        if !self.available {
            return Err(Error::Restore);
        }
        self.wait_pa()?;
        self.io.write16(i2c::BASE + 0x38, 1);
        let irq = self.io.read16(i2c::BASE + 0xc);
        if irq != 0 {
            self.io.write16(i2c::BASE + 0xc, irq);
        }
        for (off, v) in i2c::OFFSETS.into_iter().zip(self.saved) {
            if off != 8 {
                self.io.write16(i2c::BASE + off, v);
            }
        }
        self.io.write16(i2c::BASE + 8, self.saved[1]);
        self.fault()?;
        for (off, v) in i2c::OFFSETS.into_iter().zip(self.saved) {
            if self.io.read16(i2c::BASE + off) != v {
                return Err(Error::Restore);
            }
            self.fault()?;
        }
        self.pa_owned = false;
        Ok(())
    }
}
impl<I: HardwareIo> tone::Bus for Hardware<I> {
    fn ticks(&mut self) -> Result<u64> {
        self.io.counter()
    }
    fn frequency(&mut self) -> u64 {
        self.io.frequency()
    }
    fn arm(&mut self, armed: bool) {
        self.io.armed(armed, self.saved);
    }
    fn pmic_read(&mut self, reg: u16) -> Result<u16> {
        if reg & 1 != 0 {
            return Err(Error::Gate);
        }
        self.ready_pm()?;
        self.pm_owned = true;
        self.io.write32(pm::CMD, u32::from(reg >> 1) << 16);
        self.fault()?;
        let v = self.wait_pm(true, false)? as u16;
        self.io.write32(pm::CLEAR, 1);
        self.fault()?;
        self.wait_pm(false, false)?;
        self.pm_owned = false;
        Ok(v)
    }
    fn pmic_write(&mut self, reg: u16, v: u16) -> Result<()> {
        if reg & 1 != 0 {
            return Err(Error::Gate);
        }
        self.ready_pm()?;
        self.pm_owned = true;
        self.io.write32(
            pm::CMD,
            (1 << 31) | (u32::from(reg >> 1) << 16) | u32::from(v),
        );
        self.fault()?;
        self.wait_pm(false, false)?;
        self.pm_owned = false;
        Ok(())
    }
    fn amp_read(&mut self, a: u8, reg: u8) -> Result<u8> {
        self.transfer(a, reg, None)
    }
    fn amp_write(&mut self, a: u8, reg: u8, v: u8) -> Result<()> {
        self.transfer(a, reg, Some(v)).map(|_| ())
    }
    fn recover(&mut self, pmic: bool) -> Result<()> {
        self.io.fresh();
        if pmic {
            self.wait_pm(false, true)?;
            self.pm_owned = false;
        } else if self.pa_owned {
            // Reset controller only, never a GPIO recovery or amplifier reset.
            self.io.write16(i2c::BASE + 0x50, 1);
            self.fault()?;
            self.restore_pa()?;
        }
        Ok(())
    }
    fn restore(&mut self) -> Result<()> {
        // Always attempt both, even if the first is dead.
        let a = self.recover(false).and_then(|_| self.restore_pa());
        let p = self.recover(true);
        a.and(p)
    }
}
