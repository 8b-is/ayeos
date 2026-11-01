//! Board4 full-speed, EP0-only readback. No OUT payload, flash or reboot command.
//! Request layout is documented in docs/SCREEN_BOARD4_HANDOFF.md.
use crate::board_pages::crc32;

pub const VID: u16 = 0x18d1; // inherited experimental identity, not an allocated AyeOS VID
pub const PID: u16 = 0x4ee0;
pub const CHUNK: usize = 48;
#[cfg(all(feature = "alpha-report", not(feature = "tone-report")))]
const RELEASE: u8 = 0x61;
#[cfg(feature = "tone-report")]
const RELEASE: u8 = 0x62;
#[cfg(all(feature = "board6-report", not(feature = "alpha-report")))]
const RELEASE: u8 = 0x46;
#[cfg(all(feature = "audio-report", not(feature = "board6-report")))]
const RELEASE: u8 = 0x45;
#[cfg(not(feature = "audio-report"))]
const RELEASE: u8 = 0x44;
#[cfg(feature = "board6-report")]
const REPORT_ID: (&[u8; 4], &[u8; 4], &[u8; 12]) = (b"A6IF", b"A6CH", b"board6-aud\0\0");
#[cfg(all(feature = "audio-report", not(feature = "board6-report")))]
const REPORT_ID: (&[u8; 4], &[u8; 4], &[u8; 12]) = (b"A5IF", b"A5CH", b"board5-aud\0\0");
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
    RELEASE,
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
    #[cfg(feature = "audio-report")]
    report: &'a [u8],
    #[cfg(feature = "audio-report")]
    report_crc: u32,
    #[cfg(feature = "alpha-report")]
    diagnostics: &'a [u8],
    #[cfg(feature = "alpha-report")]
    diagnostics_crc: u32,
    #[cfg(feature = "tone-report")]
    tone: &'a [u8],
    #[cfg(feature = "tone-report")]
    tone_crc: u32,
    #[cfg(feature = "tone-report")]
    pub pending_tracker: Option<(u16, u16)>,
    #[cfg(feature = "tone-report")]
    pub tracker_plays: u32,
    #[cfg(feature = "tone-report")]
    pub tracker_last_cmd: u16,
    #[cfg(feature = "tone-report")]
    pub tracker_last_outcome: u32,
    #[cfg(feature = "tone-report")]
    pub tracker_last_off: u32,
    #[cfg(feature = "tone-report")]
    pub tracker_active_row: u8,
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
            #[cfg(feature = "audio-report")]
            report: &[],
            #[cfg(feature = "audio-report")]
            report_crc: 0,
            #[cfg(feature = "alpha-report")]
            diagnostics: &[],
            #[cfg(feature = "alpha-report")]
            diagnostics_crc: 0,
            #[cfg(feature = "tone-report")]
            tone: &[],
            #[cfg(feature = "tone-report")]
            tone_crc: 0,
            #[cfg(feature = "tone-report")]
            pending_tracker: None,
            #[cfg(feature = "tone-report")]
            tracker_plays: 0,
            #[cfg(feature = "tone-report")]
            tracker_last_cmd: 0,
            #[cfg(feature = "tone-report")]
            tracker_last_outcome: 0,
            #[cfg(feature = "tone-report")]
            tracker_last_off: 0,
            #[cfg(feature = "tone-report")]
            tracker_active_row: 0,
        }
    }
    #[cfg(feature = "audio-report")]
    pub fn set_report(&mut self, report: &'a [u8]) {
        self.report = report;
        self.report_crc = crc32(report);
    }
    #[cfg(feature = "alpha-report")]
    pub fn set_diagnostics(&mut self, report: &'a [u8]) {
        self.diagnostics = report;
        self.diagnostics_crc = crc32(report);
    }
    #[cfg(feature = "tone-report")]
    pub fn set_tone(&mut self, report: &'a [u8]) {
        self.tone = report;
        self.tone_crc = crc32(report);
    }
    pub fn reset(&mut self) {
        self.address = 0;
        self.configuration = 0;
        #[cfg(feature = "tone-report")]
        {
            self.pending_tracker = None;
        }
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
            #[cfg(feature = "audio-report")]
            (0xc0, 0xb2)
                if value == 0
                    && index == 0
                    && length == 64
                    && self.configuration == 1
                    && !self.report.is_empty() =>
            {
                out[..4].copy_from_slice(REPORT_ID.0);
                out[4..8].copy_from_slice(&1u32.to_le_bytes());
                out[8..12].copy_from_slice(&(self.report.len() as u32).to_le_bytes());
                out[12..16].copy_from_slice(&self.report_crc.to_le_bytes());
                out[16..20].copy_from_slice(&(CHUNK as u32).to_le_bytes());
                out[20..32].copy_from_slice(REPORT_ID.2);
                Reply::Data(64)
            }
            #[cfg(feature = "audio-report")]
            (0xc0, 0xb3) if length == 64 && self.configuration == 1 => {
                let number = u32::from(value) | (u32::from(index) << 16);
                let Some(offset) = (number as usize).checked_mul(CHUNK) else {
                    return Reply::Stall;
                };
                if offset >= self.report.len() {
                    return Reply::Stall;
                }
                let data = &self.report[offset..(offset + CHUNK).min(self.report.len())];
                out[..4].copy_from_slice(REPORT_ID.1);
                out[4..8].copy_from_slice(&(offset as u32).to_le_bytes());
                out[8..10].copy_from_slice(&(data.len() as u16).to_le_bytes());
                out[10..12].copy_from_slice(&1u16.to_le_bytes());
                out[12..16].copy_from_slice(&crc32(data).to_le_bytes());
                out[16..16 + data.len()].copy_from_slice(data);
                Reply::Data(64)
            }
            #[cfg(feature = "alpha-report")]
            (0xc0, 0xb4)
                if value == 0
                    && index == 0
                    && length == 64
                    && self.configuration == 1
                    && !self.diagnostics.is_empty() =>
            {
                out[..4].copy_from_slice(b"ADIF");
                out[4..8].copy_from_slice(&1u32.to_le_bytes());
                out[8..12].copy_from_slice(&(self.diagnostics.len() as u32).to_le_bytes());
                out[12..16].copy_from_slice(&self.diagnostics_crc.to_le_bytes());
                out[16..20].copy_from_slice(&(CHUNK as u32).to_le_bytes());
                out[20..32].copy_from_slice(b"audio-alpha1");
                Reply::Data(64)
            }
            #[cfg(feature = "alpha-report")]
            (0xc0, 0xb5) if length == 64 && self.configuration == 1 => {
                let number = u32::from(value) | (u32::from(index) << 16);
                let Some(offset) = (number as usize).checked_mul(CHUNK) else {
                    return Reply::Stall;
                };
                if offset >= self.diagnostics.len() {
                    return Reply::Stall;
                }
                let data = &self.diagnostics[offset..(offset + CHUNK).min(self.diagnostics.len())];
                out[..4].copy_from_slice(b"ADCH");
                out[4..8].copy_from_slice(&(offset as u32).to_le_bytes());
                out[8..10].copy_from_slice(&(data.len() as u16).to_le_bytes());
                out[10..12].copy_from_slice(&1u16.to_le_bytes());
                out[12..16].copy_from_slice(&crc32(data).to_le_bytes());
                out[16..16 + data.len()].copy_from_slice(data);
                Reply::Data(64)
            }
            #[cfg(feature = "tone-report")]
            (0xc0, 0xb6)
                if value == 0
                    && index == 0
                    && length == 64
                    && self.configuration == 1
                    && !self.tone.is_empty() =>
            {
                out[..4].copy_from_slice(b"ATIF");
                out[4..8].copy_from_slice(&1u32.to_le_bytes());
                out[8..12].copy_from_slice(&(self.tone.len() as u32).to_le_bytes());
                out[12..16].copy_from_slice(&self.tone_crc.to_le_bytes());
                out[16..20].copy_from_slice(&(CHUNK as u32).to_le_bytes());
                out[20..32].copy_from_slice(b"audio-tone1\0");
                Reply::Data(64)
            }
            #[cfg(feature = "tone-report")]
            (0xc0, 0xb7) if length == 64 && self.configuration == 1 => {
                let number = u32::from(value) | (u32::from(index) << 16);
                let Some(offset) = (number as usize).checked_mul(CHUNK) else {
                    return Reply::Stall;
                };
                if offset >= self.tone.len() {
                    return Reply::Stall;
                }
                let data = &self.tone[offset..(offset + CHUNK).min(self.tone.len())];
                out[..4].copy_from_slice(b"ATCH");
                out[4..8].copy_from_slice(&(offset as u32).to_le_bytes());
                out[8..10].copy_from_slice(&(data.len() as u16).to_le_bytes());
                out[10..12].copy_from_slice(&1u16.to_le_bytes());
                out[12..16].copy_from_slice(&crc32(data).to_le_bytes());
                out[16..16 + data.len()].copy_from_slice(data);
                Reply::Data(64)
            }
            #[cfg(feature = "tone-report")]
            (0xc0, 0xb8)
                if length == 64 && self.configuration == 1 && !self.tone.is_empty() =>
            {
                if value != 0 {
                    self.pending_tracker = Some((value, index));
                    self.tracker_last_cmd = value;
                }
                out[..4].copy_from_slice(b"ATRK");
                out[4..8].copy_from_slice(&1u32.to_le_bytes());
                out[8..12].copy_from_slice(&self.tracker_plays.to_le_bytes());
                out[12..14].copy_from_slice(&self.tracker_last_cmd.to_le_bytes());
                out[14] = self.tracker_active_row;
                out[15] = u8::from(self.pending_tracker.is_some());
                out[16..20].copy_from_slice(&self.tracker_last_outcome.to_le_bytes());
                out[20..24].copy_from_slice(&self.tracker_last_off.to_le_bytes());
                out[24..36].copy_from_slice(b"s3m-tracker\0");
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
    /// Bounded pre-USB diagnostics may feed the already-running watchdog.
    fn keepalive(&mut self) {}
}

/// MediaTek USB20 interrupt status is W1C, not read-to-clear. Acknowledge
/// exactly the sampled bits with the register's native width. Io supplies the
/// hardware ordering barrier and suppresses subsequent accesses after a fault.
pub fn acknowledge_interrupts(io: &mut impl Io, base: usize) -> (u8, u16) {
    let usb = io.read8(base + 0x0a);
    let tx = io.read16(base + 0x02);
    if tx != 0 {
        io.write16(base + 0x02, tx);
    }
    if usb != 0 {
        io.write8(base + 0x0a, usb);
    }
    (usb, tx)
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
    ep0_pending: bool,
    quiet_polls: u32,
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
            ep0_pending: false,
            quiet_polls: 0,
            resets: 0,
            requests: 0,
            stalls: 0,
            stopped: false,
        }
    }
    /// A scheduling hint, not a bus-idle guarantee or a calibrated time interval.
    pub fn display_idle(&self) -> bool {
        self.stopped || (self.stage == Stage::Setup && self.quiet_polls >= 256)
    }
    /// Indexed endpoint registers: INDEX=0, CSR0 at +0x12, COUNT0 at +0x18.
    /// No delay, rendering, UART, DMA, IRQ unmask or updater inside this path.
    pub fn poll(&mut self, io: &mut impl Io, base: usize) {
        if self.stopped || io.faulted() {
            self.stopped = true;
            return;
        }
        let quiet_before = self.quiet_polls;
        self.quiet_polls = 0;
        let (usb, tx) = acknowledge_interrupts(io, base);
        if io.faulted() {
            self.stopped = true;
            return;
        }
        self.ep0_pending |= tx & 1 != 0;
        io.write8(base + 0x0e, 0);
        if usb & 4 != 0 {
            self.control.reset();
            self.stage = Stage::Setup;
            self.ep0_pending = tx & 1 != 0; // cancel any previous completion
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
        // Retain the sampled completion if CSR0 is still busy. Conversely, a
        // zero/strobe-only DATAEND readback is NOT completion evidence by itself.
        if csr & (8 | 2) != 0 {
            return;
        }
        if !self.ep0_pending {
            if self.stage == Stage::Setup && usb == 0 && tx == 0 && csr == 0 {
                self.quiet_polls = quiet_before.saturating_add(1);
            }
            return;
        }
        let data_in = self.stage == Stage::DataIn;
        self.ep0_pending = false;
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
        if data_in && count == 0 {
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
        // A next SETUP can coalesce with the previous status interrupt after
        // the initial interrupt snapshot. Consume its latch BEFORE arming the
        // reply so it cannot be mistaken for that reply's completion next poll.
        let setup_irq = io.read16(base + 0x02) & 1;
        if setup_irq != 0 {
            io.write16(base + 0x02, setup_irq);
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
