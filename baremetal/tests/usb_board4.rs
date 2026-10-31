#![allow(dead_code)]
#[path = "../src/board_pages.rs"]
mod board_pages;
#[path = "../src/board_screen.rs"]
mod board_screen;
#[path = "../src/device_tree.rs"]
mod device_tree;
#[path = "../src/usb_board.rs"]
mod usb_board;
#[path = "../src/usb_control.rs"]
mod usb_control;
#[path = "../src/usb_phy.rs"]
mod usb_phy;
use std::collections::{BTreeMap, VecDeque};
use usb_control::{Control, Device, Io, Reply};
use usb_phy::{INFRA, MUSB, PHY, TOP};

fn setup(kind: u8, req: u8, val: u16, index: u16, len: u16) -> [u8; 8] {
    let [v0, v1] = val.to_le_bytes();
    let [i0, i1] = index.to_le_bytes();
    let [l0, l1] = len.to_le_bytes();
    [kind, req, v0, v1, i0, i1, l0, l1]
}
fn configured(blob: &[u8]) -> Control<'_> {
    let mut c = Control::new(blob);
    let mut b = [0; 64];
    assert_eq!(c.setup(setup(0, 5, 7, 0, 0), &mut b), Reply::Ack);
    assert_eq!(c.address, 0);
    assert_eq!(c.status_complete(), Some(7));
    assert_eq!(c.setup(setup(0, 9, 1, 0, 0), &mut b), Reply::Ack);
    assert_eq!(c.configuration, 0);
    c.status_complete();
    c
}
#[test]
fn descriptors_and_rejected_commands() {
    let mut c = configured(b"data");
    let mut b = [0; 64];
    for (val, expected) in [
        (0x100, &usb_control::DEVICE[..]),
        (0x200, &usb_control::CONFIG[..]),
    ] {
        assert_eq!(
            c.setup(setup(0x80, 6, val, 0, 255), &mut b),
            Reply::Data(expected.len())
        );
        assert_eq!(&b[..expected.len()], expected);
        assert_eq!(c.setup(setup(0x80, 6, val, 0, 8), &mut b), Reply::Data(8));
        assert_eq!(&b[..8], &expected[..8]);
    }
    for s in [
        setup(0x40, 0xa8, 0, 0, 0),
        setup(0xc0, 0xa8, 0, 0, 64),
        setup(0, 5, 128, 0, 0),
        setup(0, 9, 2, 0, 0),
        setup(0x80, 6, 0x600, 0, 10),
        setup(0xc0, 0xb0, 0, 0, 63),
        setup(0xc0, 0xb0, 1, 0, 64),
        setup(0x40, 0xb1, 0, 0, 64),
        setup(0x81, 10, 0, 1, 1),
    ] {
        assert_eq!(c.setup(s, &mut b), Reply::Stall, "{s:?}");
    }
}
#[test]
fn abort_reset_and_pending_configuration() {
    let mut c = Control::new(b"data");
    let mut b = [0; 64];
    c.setup(setup(0, 5, 42, 0, 0), &mut b);
    c.abort();
    assert_eq!(c.status_complete(), None);
    assert_eq!(c.address, 0);
    c = configured(b"data");
    c.setup(setup(0, 9, 0, 0, 0), &mut b);
    c.abort();
    c.status_complete();
    assert_eq!(c.configuration, 1);
    c.reset();
    assert_eq!((c.address, c.configuration), (0, 0));
    assert_eq!(c.setup(setup(0xc0, 0xb0, 0, 0, 64), &mut b), Reply::Stall);
}
#[test]
fn numbered_transfer_reconstructs_full_blob() {
    let blob: Vec<u8> = (0..207201).map(|n| (n % 251) as u8).collect();
    let mut c = configured(&blob);
    let mut b = [0; 64];
    let mut output = Vec::new();
    assert_eq!(
        c.setup(setup(0xc0, 0xb0, 0, 0, 64), &mut b),
        Reply::Data(64)
    );
    assert_eq!(&b[20..32], b"board4-ep0\0\0");
    assert_eq!(
        u32::from_le_bytes(b[12..16].try_into().unwrap()),
        board_pages::crc32(&blob)
    );
    for (n, expected) in blob.chunks(48).enumerate() {
        assert_eq!(
            c.setup(setup(0xc0, 0xb1, n as u16, 0, 64), &mut b),
            Reply::Data(64)
        );
        assert_eq!(
            u32::from_le_bytes(b[4..8].try_into().unwrap()) as usize,
            n * 48
        );
        assert_eq!(
            u16::from_le_bytes(b[8..10].try_into().unwrap()) as usize,
            expected.len()
        );
        assert_eq!(
            u32::from_le_bytes(b[12..16].try_into().unwrap()),
            board_pages::crc32(expected)
        );
        assert_eq!(&b[16..16 + expected.len()], expected);
        assert!(b[16 + expected.len()..].iter().all(|b| *b == 0));
        output.extend_from_slice(expected);
    }
    assert_eq!(output, blob);
    for (val, index) in [(4317, 0), (0, 1), (u16::MAX, u16::MAX)] {
        assert_eq!(
            c.setup(setup(0xc0, 0xb1, val, index, 64), &mut b),
            Reply::Stall
        );
    }
}

#[derive(Default)]
struct Mock {
    regs: BTreeMap<usize, u32>,
    fifo: VecDeque<u8>,
    sent: Vec<u8>,
    writes: Vec<(u8, usize, u32)>,
    fault: bool,
    fault_at: Option<usize>,
    ops: usize,
    stuck: Option<usize>,
}
impl Mock {
    fn gate(&mut self) -> bool {
        if self.fault {
            return false;
        }
        self.ops += 1;
        if self.fault_at == Some(self.ops) {
            self.fault = true;
            return false;
        }
        true
    }
    fn read(&mut self, a: usize) -> u32 {
        if !self.gate() {
            return 0;
        }
        if a == MUSB + 0x20 {
            return u32::from(self.fifo.pop_front().unwrap());
        }
        let v = *self.regs.get(&a).unwrap_or(&0);
        if a == MUSB + 2 || a == MUSB + 0xa {
            self.regs.insert(a, 0);
        }
        v
    }
    fn write(&mut self, w: u8, a: usize, v: u32) {
        if !self.gate() {
            return;
        }
        self.writes.push((w, a, v));
        if self.stuck == Some(a) {
            return;
        }
        if a == MUSB + 0x20 {
            self.sent.push(v as u8);
            return;
        }
        if a == MUSB + 0x12 {
            let old = *self.regs.get(&a).unwrap_or(&0);
            let next = if v == 0x80 {
                old & !0x10
            } else if v == 0x40 {
                old & !1
            } else {
                v & 0x3f
            };
            self.regs.insert(a, next);
            return;
        }
        self.regs.insert(a, v);
    }
    fn request(&mut self, s: [u8; 8]) {
        self.fifo = VecDeque::from(s);
        self.regs.insert(MUSB + 0x18, 8);
        self.regs.insert(MUSB + 0x12, 1);
        self.regs.insert(MUSB + 2, 1);
    }
    fn complete(&mut self) {
        self.regs.insert(MUSB + 0x12, 0);
        self.regs.insert(MUSB + 2, 1);
    }
}
impl Io for Mock {
    fn read8(&mut self, a: usize) -> u8 {
        self.read(a) as u8
    }
    fn read16(&mut self, a: usize) -> u16 {
        self.read(a) as u16
    }
    fn read32(&mut self, a: usize) -> u32 {
        self.read(a)
    }
    fn write8(&mut self, a: usize, v: u8) {
        self.write(8, a, u32::from(v))
    }
    fn write16(&mut self, a: usize, v: u16) {
        self.write(16, a, u32::from(v))
    }
    fn write32(&mut self, a: usize, v: u32) {
        self.write(32, a, v)
    }
    fn faulted(&self) -> bool {
        self.fault
    }
    fn settle(&mut self) {}
}
#[test]
fn status_completion_defers_address_even_after_interrupt_consumed() {
    let mut d = Device::new(b"data");
    let mut io = Mock::default();
    io.request(setup(0, 5, 42, 0, 0));
    d.poll(&mut io, MUSB);
    assert_eq!(d.control.address, 0);
    io.regs.insert(MUSB + 2, 1);
    d.poll(&mut io, MUSB); // DATAEND still set
    assert_eq!(d.control.address, 0);
    io.regs.insert(MUSB + 0x12, 0);
    d.poll(&mut io, MUSB); // no new interrupt
    assert_eq!(d.control.address, 42);
    assert!(io.writes.contains(&(8, MUSB, 42)));
}
#[test]
fn setupend_and_reset_cancel_pending_address() {
    for reset in [false, true] {
        let mut d = Device::new(b"data");
        let mut io = Mock::default();
        io.request(setup(0, 5, 42, 0, 0));
        d.poll(&mut io, MUSB);
        if reset {
            io.regs.insert(MUSB + 0xa, 4);
            io.regs.insert(MUSB + 0x12, 0);
        } else {
            io.regs.insert(MUSB + 0x12, 0x10);
        }
        d.poll(&mut io, MUSB);
        io.complete();
        d.poll(&mut io, MUSB);
        assert_eq!(d.control.address, 0);
        assert!(!io.writes.contains(&(8, MUSB, 42)));
    }
}
#[test]
fn zero_out_status_and_immediate_next_setup_do_not_stall() {
    for zero in [false, true] {
        let mut d = Device::new(b"data");
        let mut io = Mock::default();
        io.request(setup(0x80, 6, 0x100, 0, 18));
        d.poll(&mut io, MUSB);
        assert_eq!(io.sent, usb_control::DEVICE);
        d.poll(&mut io, MUSB);
        assert_eq!(d.requests, 1); // TX/DATAEND active
        if zero {
            io.regs.insert(MUSB + 0x12, 1);
            io.regs.insert(MUSB + 0x18, 0);
            io.regs.insert(MUSB + 2, 1);
            d.poll(&mut io, MUSB);
        }
        io.request(setup(0x80, 6, 0x200, 0, 18));
        d.poll(&mut io, MUSB);
        assert_eq!(d.stalls, 0);
        assert_eq!(d.requests, 2);
    }
}
#[test]
fn pending_address_completes_before_following_setup() {
    let mut d = Device::new(b"data");
    let mut io = Mock::default();
    io.request(setup(0, 5, 42, 0, 0));
    d.poll(&mut io, MUSB);
    io.request(setup(0, 9, 1, 0, 0));
    d.poll(&mut io, MUSB);
    assert_eq!(d.control.address, 42);
    assert_eq!(d.control.configuration, 0);
    io.complete();
    d.poll(&mut io, MUSB);
    assert_eq!(d.control.configuration, 1);
}
#[test]
fn unmatched_profile_and_faults_stop_hardware_writes() {
    let mut io = Mock::default();
    assert_eq!(
        usb_phy::initialize(&mut io, false),
        usb_phy::Init::FdtMismatch
    );
    assert_eq!(io.ops, 0);
    for n in 1..70 {
        let mut io = Mock::default();
        io.regs.insert(MUSB + 0x6c, 0x1900);
        io.fault_at = Some(n);
        let init = usb_phy::initialize(&mut io, true);
        if io.fault {
            assert_eq!(init, usb_phy::Init::Fault);
            assert_eq!(io.ops, n);
        }
        let mut d = Device::new(b"data");
        let before = io.ops;
        d.poll(&mut io, MUSB);
        if io.fault && before == n {
            assert_eq!(io.ops, n);
            assert!(d.stopped);
        }
    }
}
#[test]
fn register_widths_readbacks_and_full_speed_attach() {
    let mut io = Mock::default();
    io.regs.insert(MUSB + 0x6c, 0x1900);
    assert_eq!(usb_phy::initialize(&mut io, true), usb_phy::Init::Ready);
    assert!(!io.writes.contains(&(8, MUSB + 1, 0x40)));
    assert_eq!(usb_phy::connect(&mut io), usb_phy::Init::Attached);
    assert_eq!(io.writes.last(), Some(&(8, MUSB + 1, 0x40)));
    for &(width, addr, _) in &io.writes {
        if addr >= PHY && addr < PHY + 0x100
            || addr >= TOP && addr < TOP + 0x100
            || addr >= INFRA && addr < INFRA + 0x100
        {
            assert_eq!(width, 32);
        }
        if addr == MUSB + 0x12 || addr == MUSB + 6 || addr == MUSB + 8 {
            assert_eq!(width, 16);
        }
        assert_ne!(addr, MUSB + 0x102);
    }
    let mut io = Mock::default();
    io.regs.insert(TOP + 0x90, 1 << 23);
    assert_eq!(
        usb_phy::initialize(&mut io, true),
        usb_phy::Init::ClockReadback
    );
    assert!(io.writes.iter().all(|(_, a, _)| *a < MUSB));
    let mut io = Mock::default();
    assert_eq!(
        usb_phy::initialize(&mut io, true),
        usb_phy::Init::ControllerAbsent
    );
    let mut io = Mock::default();
    io.regs.insert(MUSB + 0x6c, 0x1900);
    io.regs.insert(PHY + 0x68, 1 << 26);
    io.stuck = Some(PHY + 0x68);
    assert_eq!(
        usb_phy::initialize(&mut io, true),
        usb_phy::Init::PhyReadback
    );
    assert!(!io.writes.contains(&(8, MUSB + 1, 0x40)));
    let mut io = Mock::default();
    io.stuck = Some(MUSB + 1);
    assert_eq!(usb_phy::connect(&mut io), usb_phy::Init::ConnectReadback);
}

struct Fdt {
    s: Vec<u8>,
    t: Vec<u8>,
}
impl Fdt {
    fn word(&mut self, v: u32) {
        self.s.extend(v.to_be_bytes());
    }
    fn pad(&mut self) {
        while self.s.len() % 4 != 0 {
            self.s.push(0);
        }
    }
    fn begin(&mut self, n: &str) {
        self.word(1);
        self.s.extend(n.as_bytes());
        self.s.push(0);
        self.pad();
    }
    fn end(&mut self) {
        self.word(2);
    }
    fn prop(&mut self, n: &str, v: &[u8]) {
        self.word(3);
        self.word(v.len() as u32);
        self.word(self.t.len() as u32);
        self.t.extend(n.as_bytes());
        self.t.push(0);
        self.s.extend(v);
        self.pad();
    }
    fn cells(&mut self, n: &str, v: &[u32]) {
        self.prop(
            n,
            &v.iter().flat_map(|v| v.to_be_bytes()).collect::<Vec<_>>(),
        );
    }
    fn finish(mut self) -> Vec<u8> {
        self.end();
        self.word(9);
        let mut b = vec![0; 56];
        b.extend(&self.s);
        b.extend(&self.t);
        for (i, v) in [
            0xd00dfeed,
            b.len() as u32,
            56,
            (56 + self.s.len()) as u32,
            40,
            17,
            16,
            0,
            self.t.len() as u32,
            self.s.len() as u32,
        ]
        .iter()
        .enumerate()
        {
            b[i * 4..i * 4 + 4].copy_from_slice(&v.to_be_bytes());
        }
        b
    }
}
fn fixture(mode: u8) -> Vec<u8> {
    let mut f = Fdt {
        s: vec![],
        t: vec![],
    };
    f.begin("");
    f.prop(
        "compatible",
        if mode == 1 {
            b"other\0"
        } else {
            b"mediatek,MT6768\0"
        },
    );
    f.cells("#address-cells", &[2]);
    f.cells("#size-cells", &[2]);
    f.begin("soc");
    f.cells("#address-cells", &[2]);
    f.cells("#size-cells", &[2]);
    if mode != 2 {
        f.prop("ranges", if mode == 3 { b"bad" } else { b"" });
    }
    if mode == 4 {
        f.prop("status", b"disabled\0");
    }
    for _ in 0..if mode == 5 { 2 } else { 1 } {
        f.begin("usb0@11200000");
        f.prop("compatible", b"mediatek,mt6768-usb20\0");
        f.cells(
            "reg",
            &[0, 0x11200000, 0, 0x10000, 0, 0x11cc0000, 0, 0x10000],
        );
        f.cells("phys", &[if mode == 6 { 9 } else { 8 }, 3]);
        f.end();
    }
    f.begin("usb-phy");
    f.prop("compatible", b"mediatek,generic-tphy-v1\0");
    f.cells("#address-cells", &[2]);
    f.cells("#size-cells", &[2]);
    f.prop("ranges", b"");
    f.begin("port");
    f.cells(
        "reg",
        &[0, if mode == 7 { 0x11cc0000 } else { 0x11cc0800 }, 0, 0x100],
    );
    f.cells("#phy-cells", &[1]);
    f.cells("phandle", &[8]);
    f.end();
    f.end();
    for (name, compat, base) in [
        ("infra", &b"mediatek,infracfg_ao\0"[..], 0x10001000),
        ("top", &b"mediatek,topckgen\0"[..], 0x10000000),
    ] {
        f.begin(name);
        f.prop("compatible", compat);
        f.cells("reg", &[0, base, 0, 0x1000]);
        f.end();
    }
    f.end();
    f.finish()
}
#[test]
fn fdt_discovery_requires_soc_translation_enabled_nodes_and_linked_port() {
    assert!(usb_board::discover(&fixture(0)).unwrap().ready());
    for mode in 1..=7 {
        assert!(
            !usb_board::discover(&fixture(mode)).unwrap().ready(),
            "mode {mode}"
        );
    }
    let blob = fixture(0);
    for n in 0..blob.len() {
        assert!(usb_board::discover(&blob[..n]).is_err());
    }
}
