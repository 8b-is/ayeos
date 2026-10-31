//! Board4 full-speed, EP0-only readback. No OUT payload, flash or reboot command.
//! Request layout is documented in docs/SCREEN_BOARD4_HANDOFF.md.
use crate::board_pages::crc32;

pub const VID: u16 = 0x18d1; // inherited experimental identity, not an allocated AyeOS VID
pub const PID: u16 = 0x4ee0;
pub const CHUNK: usize = 48;
pub const DEVICE: [u8; 18] = [
    18,
    1,
    0x10,
    1,
    0xff,
    0xa8,
    4,
    64,
    VID as u8,
    (VID >> 8) as u8,
    PID as u8,
    (PID >> 8) as u8,
    0x44,
    5,
    0,
    0,
    0,
    1,
];
pub const CONFIG: [u8; 18] = [
    9, 2, 18, 0, 1, 1, 0, 0x80, 250, 9, 4, 0, 0, 0, 0xff, 0xa8, 4, 0,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reply {
    Data(usize),
    Ack,
    Stall,
}

pub struct Control<'a> {
    blob: &'a [u8],
    crc: u32,
    pub address: u8,
    pub configuration: u8,
    pending_address: Option<u8>,
    pending_configuration: Option<u8>,
}
impl<'a> Control<'a> {
    pub fn new(blob: &'a [u8]) -> Self {
        Self {
            blob,
            crc: crc32(blob),
            address: 0,
            configuration: 0,
            pending_address: None,
            pending_configuration: None,
        }
    }
    pub fn reset(&mut self) {
        self.address = 0;
        self.configuration = 0;
        self.abort();
    }
    pub fn abort(&mut self) {
        self.pending_address = None;
        self.pending_configuration = None;
    }
    /// Called only after MUSB reports completion of the IN status stage.
    pub fn status_complete(&mut self) -> Option<u8> {
        let result = self.pending_address.take();
        if let Some(address) = result {
            self.address = address;
        }
        if let Some(configuration) = self.pending_configuration.take() {
            self.configuration = configuration;
        }
        result
    }
    pub fn setup(&mut self, s: [u8; 8], out: &mut [u8; 64]) -> Reply {
        self.abort();
        out.fill(0);
        let value = u16::from_le_bytes([s[2], s[3]]);
        let index = u16::from_le_bytes([s[4], s[5]]);
        let length = usize::from(u16::from_le_bytes([s[6], s[7]]));
        match (s[0], s[1]) {
            (0x80, 6) if index == 0 => {
                let data: &[u8] = match value {
                    0x0100 => &DEVICE,
                    0x0200 => &CONFIG,
                    0x0300 => &[4, 3, 9, 4],
                    _ => return Reply::Stall,
                };
                let n = length.min(data.len());
                out[..n].copy_from_slice(&data[..n]);
                Reply::Data(n)
            }
            (0, 5) if value <= 127 && index == 0 && length == 0 && self.configuration == 0 => {
                self.pending_address = Some(value as u8);
                Reply::Ack
            }
            (0, 9) if value <= 1 && index == 0 && length == 0 && self.address != 0 => {
                self.pending_configuration = Some(value as u8);
                Reply::Ack
            }
            (0x80, 8) if value == 0 && index == 0 && length == 1 => {
                out[0] = self.configuration;
                Reply::Data(1)
            }
            (0x80, 0) if value == 0 && index == 0 && length == 2 => Reply::Data(2),
            (0x81, 0) if value == 0 && index == 0 && length == 2 && self.configuration == 1 => {
                Reply::Data(2)
            }
            (0x82, 0) if value == 0 && (index == 0 || index == 0x80) && length == 2 => {
                Reply::Data(2)
            }
            (0x81, 10) if value == 0 && index == 0 && length == 1 && self.configuration == 1 => {
                Reply::Data(1)
            }
            (1, 11) if value == 0 && index == 0 && length == 0 && self.configuration == 1 => {
                Reply::Ack
            }
            (0xc0, 0xb0) if value == 0 && index == 0 && length == 64 && self.configuration == 1 => {
                out[..4].copy_from_slice(b"A4IF");
                out[4..8].copy_from_slice(&1u32.to_le_bytes());
                out[8..12].copy_from_slice(&(self.blob.len() as u32).to_le_bytes());
                out[12..16].copy_from_slice(&self.crc.to_le_bytes());
                out[16..20].copy_from_slice(&(CHUNK as u32).to_le_bytes());
                out[20..32].copy_from_slice(b"board4-ep0\0\0");
                Reply::Data(64)
            }
            (0xc0, 0xb1) if length == 64 && self.configuration == 1 => {
                let number = u32::from(value) | (u32::from(index) << 16);
                let Some(offset) = (number as usize).checked_mul(CHUNK) else {
                    return Reply::Stall;
                };
                if offset >= self.blob.len() {
                    return Reply::Stall;
                }
                let end = (offset + CHUNK).min(self.blob.len());
                let data = &self.blob[offset..end];
                out[..4].copy_from_slice(b"A4CH");
                out[4..8].copy_from_slice(&(offset as u32).to_le_bytes());
                out[8..10].copy_from_slice(&(data.len() as u16).to_le_bytes());
                out[10..12].copy_from_slice(&1u16.to_le_bytes());
                out[12..16].copy_from_slice(&crc32(data).to_le_bytes());
                out[16..16 + data.len()].copy_from_slice(data);
                Reply::Data(64)
            }
            _ => Reply::Stall,
        }
    }
}

/// MMIO boundary, mocked on the host. Implementations MUST suppress all further
/// accesses after a fault, including within a multi-access initialization step.
pub trait Io {
    fn read8(&mut self, address: usize) -> u8;
    fn read16(&mut self, address: usize) -> u16;
    fn read32(&mut self, address: usize) -> u32;
    fn write8(&mut self, address: usize, value: u8);
    fn write16(&mut self, address: usize, value: u16);
    fn write32(&mut self, address: usize, value: u32);
    fn faulted(&self) -> bool;
    fn settle(&mut self);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    Setup,
    DataIn,
    StatusIn,
}

pub struct Device<'a> {
    pub control: Control<'a>,
    stage: Stage,
    pub resets: u32,
    pub requests: u32,
    pub stalls: u32,
    pub stopped: bool,
}
impl<'a> Device<'a> {
    pub fn new(blob: &'a [u8]) -> Self {
        Self {
            control: Control::new(blob),
            stage: Stage::Setup,
            resets: 0,
            requests: 0,
            stalls: 0,
            stopped: false,
        }
    }
    /// Indexed endpoint registers: INDEX=0, CSR0 at +0x12, COUNT0 at +0x18.
    /// No delay, rendering, UART, DMA, IRQ unmask or updater inside this path.
    pub fn poll(&mut self, io: &mut impl Io, base: usize) {
        if self.stopped || io.faulted() {
            self.stopped = true;
            return;
        }
        let usb = io.read8(base + 0x0a);
        let _ = io.read16(base + 0x02); // clear latched EP0 interrupt
        io.write8(base + 0x0e, 0);
        if usb & 4 != 0 {
            self.control.reset();
            self.stage = Stage::Setup;
            self.resets = self.resets.saturating_add(1);
            io.write8(base, 0);
        }
        let mut csr = io.read16(base + 0x12);
        if io.faulted() {
            self.stopped = true;
            return;
        }
        if csr & 0x10 != 0 {
            // host ended/aborted previous transfer
            io.write16(base + 0x12, 0x80);
            self.control.abort();
            self.stage = Stage::Setup;
            csr = io.read16(base + 0x12);
        }
        if csr & 4 != 0 {
            io.write16(base + 0x12, csr & !4);
            self.control.abort();
            self.stage = Stage::Setup;
            csr = io.read16(base + 0x12);
        }
        // DATAEND is cleared by hardware only after the status handshake. The
        // interrupt may have been consumed during an earlier poll: use CSR0.
        if csr & (8 | 2) != 0 {
            return;
        }
        if self.stage == Stage::StatusIn {
            if let Some(address) = self.control.status_complete() {
                io.write8(base, address);
            }
            self.stage = Stage::Setup;
        }
        if csr & 1 == 0 {
            self.stage = Stage::Setup;
            return;
        }
        let count = io.read8(base + 0x18);
        // A zero-length OUT status packet may be visible before auto-ack.
        if self.stage == Stage::DataIn && count == 0 {
            io.write16(base + 0x12, 0x40);
            self.stage = Stage::Setup;
            return;
        }
        if count != 8 || csr & 2 != 0 {
            io.write16(base + 0x12, 0x60);
            self.control.abort();
            self.stage = Stage::Setup;
            self.stalls = self.stalls.saturating_add(1);
            return;
        }
        let mut setup = [0; 8];
        for b in &mut setup {
            *b = io.read8(base + 0x20);
        }
        if io.faulted() {
            self.stopped = true;
            return;
        }
        let mut payload = [0; 64];
        self.requests = self.requests.saturating_add(1);
        match self.control.setup(setup, &mut payload) {
            Reply::Data(n) if n > 0 => {
                io.write16(base + 0x12, 0x40);
                for b in &payload[..n] {
                    io.write8(base + 0x20, *b);
                }
                io.write16(base + 0x12, 0x0a);
                self.stage = Stage::DataIn;
            }
            Reply::Ack | Reply::Data(_) => {
                let csr = if setup[0] & 0x80 != 0 { 0x4a } else { 0x48 };
                io.write16(base + 0x12, csr);
                self.stage = Stage::StatusIn;
            }
            Reply::Stall => {
                io.write16(base + 0x12, 0x60);
                self.stage = Stage::Setup;
                self.stalls = self.stalls.saturating_add(1);
            }
        }
        if io.faulted() {
            self.stopped = true;
        }
    }
}
