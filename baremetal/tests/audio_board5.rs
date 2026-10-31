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
#[path = "../src/usb_control.rs"]
mod usb_control;
use audio_probe::{Status, BASE, INFRA, PIN, TOP};
use std::collections::BTreeMap;
use usb_control::{Control, Io, Reply};

// Synthetic, public fixture. No private phone FDT is embedded in source/tests.
struct Fdt {
    s: Vec<u8>,
    names: Vec<u8>,
}
impl Fdt {
    fn new() -> Self {
        Self {
            s: vec![],
            names: vec![],
        }
    }
    fn word(&mut self, v: u32) {
        self.s.extend(v.to_be_bytes());
    }
    fn begin(&mut self, n: &str) {
        self.word(1);
        self.s.extend(n.as_bytes());
        self.s.push(0);
        while self.s.len() % 4 != 0 {
            self.s.push(0);
        }
    }
    fn end(&mut self) {
        self.word(2);
    }
    fn prop(&mut self, n: &str, v: &[u8]) {
        let off = self.names.len();
        self.names.extend(n.as_bytes());
        self.names.push(0);
        self.word(3);
        self.word(v.len() as u32);
        self.word(off as u32);
        self.s.extend(v);
        while self.s.len() % 4 != 0 {
            self.s.push(0);
        }
    }
    fn cells(&mut self, n: &str, v: &[u32]) {
        let mut b = vec![];
        for x in v {
            b.extend(x.to_be_bytes());
        }
        self.prop(n, &b);
    }
    fn finish(mut self) -> Vec<u8> {
        self.word(9);
        let mut b = vec![0; 56];
        for (i, w) in [
            0xd00dfeed,
            56 + self.s.len() + self.names.len(),
            56,
            56 + self.s.len(),
            40,
            17,
            16,
            0,
            self.names.len(),
            self.s.len(),
        ]
        .into_iter()
        .enumerate()
        {
            b[i * 4..i * 4 + 4].copy_from_slice(&(w as u32).to_be_bytes());
        }
        b.extend(self.s);
        b.extend(self.names);
        b
    }
}
fn fixture(change: u8) -> Vec<u8> {
    let mut f = Fdt::new();
    f.begin("");
    f.prop("compatible", b"mediatek,MT6768\0");
    f.cells("#address-cells", &[2]);
    f.cells("#size-cells", &[if change == 1 { 1 } else { 2 }]);
    for (name, compat, addr) in [
        (
            "infracfg_ao@10001000",
            &b"mediatek,infracfg_ao\0"[..],
            0x10001000,
        ),
        ("topckgen@10000000", &b"mediatek,topckgen\0"[..], 0x10000000),
        ("gpio@10005000", &b"mediatek,gpio\0"[..], 0x10005000),
    ] {
        f.begin(name);
        f.prop("compatible", compat);
        f.cells("reg", &[0, addr, 0, 0x1000]);
        if addr == 0x10005000 {
            f.cells("gpio_init_default", &[0, 0, 0]);
        }
        if addr == 0x10001000 {
            f.cells("phandle", &[42]);
            f.cells("#clock-cells", &[1]);
        }
        f.end();
    }
    f.begin("i2c@11004000");
    f.prop("compatible", b"mediatek,mt6768-i2c\0");
    f.cells(
        "reg",
        &[
            0,
            if change == 2 { 0x11003000 } else { 0x11004000 },
            0,
            0x1000,
            0,
            0x11000680,
            0,
            0x180,
        ],
    );
    f.cells("clocks", &[if change == 3 { 7 } else { 42 }, 11, 42, 38]);
    f.prop("clock-names", b"main\0dma\0");
    f.cells("#address-cells", &[1]);
    f.cells("#size-cells", &[0]);
    f.cells("clock-div", &[1]);
    f.cells("clock-frequency", &[400000]);
    f.prop("mediatek,use-open-drain", b"");
    f.prop("mediatek,control-irq-sel", b"");
    f.prop("mediatek,control-rs-stop", b"");
    if change == 4 {
        f.prop("status", b"disabled\0");
    }
    for (i, n, a) in [
        (0, "aw87xxx_smartpa_0@58", 0x58),
        (1, "aw87xxx_smartpa_1@59", 0x59),
    ] {
        f.begin(n);
        f.cells("reg", &[a]);
        f.cells("dev_index", &[i]);
        f.prop("compatible", b"awinic,aw87xxx_pa\0");
        if change == 5 {
            f.cells("reset-gpios", &[42, 1, 0]);
        }
        f.end();
    }
    f.end();
    if change == 6 {
        f.begin("duplicate");
        f.cells("phandle", &[42]);
        f.end();
    }
    f.end();
    f.finish()
}

#[derive(Default)]
struct Mock {
    regs: BTreeMap<usize, u32>,
    writes: Vec<(u8, usize, u32)>,
    ops: usize,
    fault_at: Option<usize>,
    fault: bool,
    result: u16,
    fifo: u16,
    starts: usize,
    stuck_reset: bool,
    drop_restore: bool,
}
impl Mock {
    fn ready() -> Self {
        let mut m = Self {
            result: 1,
            fifo: 0x0110, // one RX byte; not a literal count register
            ..Self::default()
        };
        m.regs.insert(PIN, 0x110);
        m.regs.insert(BASE + 0x20, 0x10);
        m.regs.insert(BASE + 0x34, 3);
        m.regs.insert(BASE + 0xf4, 0x2201); // captured empty AP FIFO state
        m
    }
    fn gate(&mut self) -> bool {
        if self.fault {
            return false;
        }
        self.ops += 1;
        if Some(self.ops) == self.fault_at {
            self.fault = true;
            false
        } else {
            true
        }
    }
    fn read(&mut self, a: usize) -> u32 {
        if !self.gate() {
            return 0;
        }
        if a == BASE {
            self.regs.insert(BASE + 0xf4, 0x1101); // equal read/write pointers, empty
            return if self.starts == 1 { 0x39 } else { 0x59 };
        }
        *self.regs.get(&a).unwrap_or(&0)
    }
    fn write(&mut self, w: u8, a: usize, v: u32) {
        if !self.gate() {
            return;
        }
        self.writes.push((w, a, v));
        assert!(
            (BASE..BASE + 0x100).contains(&a),
            "no clocks, GPIO, DMA, codec or charger writes"
        );
        if a == BASE + 0xc {
            assert_eq!(w, 16);
            let old = *self.regs.get(&a).unwrap_or(&0);
            self.regs.insert(a, old & !v);
        } else if a == BASE + 0x24 {
            self.starts += 1;
            self.regs.insert(a, if self.result == 0 { 1 } else { 0 });
            self.regs.insert(BASE + 0xc, u32::from(self.result));
            self.regs.insert(BASE + 0xf4, u32::from(self.fifo));
        } else if a == BASE + 0x38 {
            self.regs.insert(BASE + 0xf4, 1);
        } else if a == BASE + 0x50 {
            if !self.stuck_reset {
                self.regs.insert(BASE + 0x24, 0);
            }
            self.regs.insert(BASE + 0xf4, 1);
        } else if a != BASE {
            if !(self.drop_restore && a == BASE + 0x10 && v == 0) {
                self.regs.insert(a, v);
            }
        }
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
        self.write(8, a, u32::from(v));
    }
    fn write16(&mut self, a: usize, v: u16) {
        self.write(16, a, u32::from(v));
    }
    fn write32(&mut self, a: usize, v: u32) {
        self.write(32, a, v);
    }
    fn faulted(&self) -> bool {
        self.fault
    }
    fn settle(&mut self) {
        panic!("no unbounded/instruction delay");
    }
}
#[test]
fn profile_resolves_handles_and_rejects_wrong_wiring_before_any_io() {
    assert!(audio_board::matches(&fixture(0)).unwrap());
    for variant in 1..=6 {
        let b = fixture(variant);
        assert!(!audio_board::matches(&b).unwrap());
        let mut m = Mock::ready();
        let r = audio_probe::run(&mut m, &b);
        assert_eq!(r.words[4], Status::FdtMismatch as u32);
        assert_eq!(m.ops, 0);
    }
}
#[test]
fn two_combined_byte_reads_restore_configuration_and_do_not_write_chip_values() {
    let mut m = Mock::ready();
    let before = m.regs.clone();
    let r = audio_probe::run(&mut m, &fixture(0));
    assert_eq!(r.words[4], 1);
    assert_eq!(r.words[33], 0x39);
    assert_eq!(r.words[38], 0x59);
    assert_eq!(r.words[42], 1);
    for off in audio_probe::OFFSETS {
        assert_eq!(
            *m.regs.get(&(BASE + off)).unwrap_or(&0),
            *before.get(&(BASE + off)).unwrap_or(&0)
        );
    }
    assert_eq!(
        m.writes
            .iter()
            .filter(|x| x.1 == BASE)
            .copied()
            .collect::<Vec<_>>(),
        vec![(8, BASE, 0), (8, BASE, 0)]
    );
    let addrs: Vec<_> = m
        .writes
        .iter()
        .filter(|x| x.1 == BASE + 4)
        .map(|x| x.2)
        .collect();
    assert_eq!(&addrs[..2], &[0xb0, 0xb2]);
    assert!(m
        .writes
        .iter()
        .filter(|x| x.1 == BASE + 0x10)
        .all(|x| x.2 & 0x304 == 0));
}
#[test]
fn missing_clocks_pins_or_inherited_config_never_start_a_transaction() {
    for (addr, value, status) in [
        (TOP, 0x80, Status::ClockOff),
        (INFRA, 0x80, Status::ClockOff),
        (PIN, 0, Status::Pins),
        (BASE + 0x24, 1, Status::Busy),
        (BASE + 0xf4, 0x0110, Status::Busy),
        (BASE + 0x20, 0, Status::Config),
        (BASE + 0x30, 1, Status::Config),
        (BASE + 0x34, 0, Status::Config),
    ] {
        let mut m = Mock::ready();
        m.regs.insert(addr, value);
        let r = audio_probe::run(&mut m, &fixture(0));
        assert_eq!(r.words[4], status as u32);
        assert!(m.writes.is_empty());
        if status != Status::ClockOff {
            assert!(r.words[15..32].iter().all(|x| *x != u32::MAX));
        } else {
            assert!(r.words[11..32].iter().all(|x| *x == u32::MAX));
        }
    }
}
#[test]
fn errors_stop_the_second_probe_and_restore_after_bounded_reset() {
    for (irq, fifo, status) in [
        (0, 0, Status::Timeout),
        (2, 0, Status::Nack),
        (8, 0, Status::Arbitration),
        (0x201, 0x0110, Status::Controller),
        (1, 0x2201, Status::Fifo),
        (1, 0x0220, Status::Fifo),
    ] {
        let mut m = Mock::ready();
        m.result = irq;
        m.fifo = fifo;
        let r = audio_probe::run(&mut m, &fixture(0));
        assert_eq!(r.words[4], status as u32);
        assert_eq!(r.words[32], status as u32);
        assert_eq!(r.words[37], 0);
        assert_eq!(m.starts, 1);
        assert_eq!(r.words[42], 1);
        assert!(m.ops < (audio_probe::BUDGET as usize) + 300);
        assert!(m.writes.contains(&(16, BASE + 0x50, 1)));
    }
}
#[test]
fn captured_idle_state_and_ap_fifo_count_bits_drive_the_probe() {
    let mut m = Mock::ready();
    m.regs.insert(TOP, 0x01030202);
    m.regs.insert(INFRA, 0);
    m.regs.insert(PIN, 0x10000111);
    m.regs.insert(BASE + 0x24, 0);
    m.regs.insert(BASE + 0xc, 1); // inherited completion, source transaction unknown
    m.regs.insert(BASE + 0xf4, 0x2201);
    m.regs.insert(BASE + 0xe4, 0x0300);
    let r = audio_probe::run(&mut m, &fixture(0));
    assert_eq!(r.words[4], Status::Complete as u32);
    assert_eq!(
        &r.words[8..15],
        &[0x01030202, 0, 0x10000111, 0, 1, 0x2201, 0x0300]
    );
    assert_eq!(m.starts, 2);
    assert_eq!(r.words[35], 1);
    assert_eq!(r.words[40], 1);
    // Status and pointer bits must not enter the count, including count=8.
    for (raw, count) in [
        (0x0001, 0),
        (0x2201, 0),
        (0x0110, 1),
        (0x7810, 1),
        (0x0882, 8),
    ] {
        let mut m = Mock::ready();
        m.fifo = raw;
        let r = audio_probe::run(&mut m, &fixture(0));
        assert_eq!(r.words[35], count);
        assert_eq!(
            r.words[4],
            if count == 1 {
                Status::Complete as u32
            } else {
                Status::Fifo as u32
            }
        );
    }
}
#[test]
fn fault_at_every_success_path_access_stops_all_later_mmio() {
    let b = fixture(0);
    let mut clean = Mock::ready();
    audio_probe::run(&mut clean, &b);
    for at in 1..=clean.ops {
        let mut m = Mock::ready();
        m.fault_at = Some(at);
        let r = audio_probe::run(&mut m, &b);
        assert_eq!(r.words[4], Status::Fault as u32, "access {at}");
        assert_eq!(m.ops, at);
    }
}
#[test]
fn failed_restoration_is_not_reported_as_success() {
    let mut m = Mock::ready();
    m.result = 0;
    m.stuck_reset = true;
    let r = audio_probe::run(&mut m, &fixture(0));
    assert_eq!(r.words[4], Status::Restore as u32);
    assert_eq!(r.words[42], 0);
    assert!(m.ops < (audio_probe::BUDGET as usize) + 400);
    let mut m = Mock::ready();
    m.drop_restore = true;
    let r = audio_probe::run(&mut m, &fixture(0));
    assert_eq!(r.words[4], Status::Restore as u32);
    assert_eq!(r.words[42], 0);
    let mut m = Mock::ready();
    m.fault_at = Some(1);
    let r = audio_probe::run(&mut m, &fixture(0));
    assert!(r.words[8..32].iter().all(|x| *x == u32::MAX));
}
fn setup(kind: u8, request: u8, value: u16, length: u16) -> [u8; 8] {
    let v = value.to_le_bytes();
    let n = length.to_le_bytes();
    [kind, request, v[0], v[1], 0, 0, n[0], n[1]]
}
#[test]
fn report_transport_is_configured_read_only_bounded_and_reset_stable() {
    let blob = fixture(0);
    let report = audio_probe::run(&mut Mock::ready(), &blob).encode();
    let mut c = Control::new(&blob);
    c.set_report(&report);
    let mut out = [0; 64];
    assert_eq!(c.setup(setup(0xc0, 0xb2, 0, 64), &mut out), Reply::Stall);
    c.setup(setup(0, 5, 7, 0), &mut out);
    c.status_complete();
    c.setup(setup(0, 9, 1, 0), &mut out);
    c.status_complete();
    assert_eq!(c.setup(setup(0xc0, 0xb2, 0, 64), &mut out), Reply::Data(64));
    assert_eq!(&out[..4], b"A5IF");
    let mut all: Vec<u8> = vec![];
    for n in 0..4 {
        assert_eq!(c.setup(setup(0xc0, 0xb3, n, 64), &mut out), Reply::Data(64));
        assert_eq!(&out[..4], b"A5CH");
        assert_eq!(
            u32::from_le_bytes(out[12..16].try_into().unwrap()),
            board_pages::crc32(&out[16..])
        );
        all.extend(&out[16..]);
    }
    assert_eq!(all, report);
    if let Ok(path) = std::env::var("BOARD5_TEST_REPORT") {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .unwrap();
        f.write_all(&report).unwrap();
    }
    for s in [
        setup(0x40, 0xb3, 0, 64),
        setup(0xc0, 0xb3, 4, 64),
        setup(0xc0, 0xb2, 1, 64),
        setup(0xc0, 0xb2, 0, 63),
    ] {
        assert_eq!(c.setup(s, &mut out), Reply::Stall);
    }
    c.reset();
    assert_eq!(c.setup(setup(0xc0, 0xb3, 0, 64), &mut out), Reply::Stall);
}
#[test]
#[ignore = "requires explicitly supplied private capture path"]
fn live_fdt_matches_without_embedding_private_data() {
    let path = std::env::var("BOARD5_PRIVATE_FDT").unwrap();
    let b = std::fs::read(path).unwrap();
    assert!(audio_board::matches(&b).unwrap());
}
