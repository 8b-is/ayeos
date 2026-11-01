use super::*;
use pmic_probe as p;

struct Hardware {
    i2c: Mock,
    regs: BTreeMap<usize, u32>,
    pointer: u8,
    commands: Vec<u16>,
    ops: usize,
    fault_at: usize,
    fault: bool,
    fail_i2c_at: usize,
    fail_irq: u16,
    timeout_pmic: bool,
    ack_stuck: bool,
    read_test: u16,
    swcid: u16,
    // Inject SYNC gaps only in otherwise idle RDATA, at a selected command boundary.
    sync_gap: Option<(usize, usize, usize)>, // completed commands, idle samples to skip, gap length
    gap_value: u32,
    last_rdata: u32,
    clears: usize,
}
impl Hardware {
    fn ready() -> Self {
        let mut regs = BTreeMap::new();
        for off in [4, 0x9c, 0xa0] {
            regs.insert(p::BASE + off, 1);
        }
        regs.insert(p::RDATA, p::INIT | p::SYNC);
        Self {
            i2c: Mock::ready(),
            regs,
            pointer: 0,
            commands: vec![],
            ops: 0,
            fault_at: usize::MAX,
            fault: false,
            fail_i2c_at: usize::MAX,
            fail_irq: 2,
            timeout_pmic: false,
            ack_stuck: false,
            read_test: 0x5aa5,
            swcid: 0x5810,
            sync_gap: None,
            gap_value: 0x8060_0010, // operator-reported FSM0/INIT1/SYNC0
            last_rdata: 0,
            clears: 0,
        }
    }
    fn gate(&mut self) -> bool {
        if self.fault {
            return false;
        }
        self.ops += 1;
        self.fault = self.ops == self.fault_at;
        !self.fault
    }
}
impl Io for Hardware {
    fn keepalive(&mut self) {
        self.gate();
    }
    fn faulted(&self) -> bool {
        self.fault
    }
    fn settle(&mut self) {
        panic!("no delay");
    }
    fn read8(&mut self, a: usize) -> u8 {
        if !self.gate() {
            return 0;
        }
        assert_eq!(a, BASE);
        self.i2c.read8(a);
        if self.pointer == 0 {
            0x23
        } else {
            self.pointer ^ if self.i2c.starts <= 128 { 0x58 } else { 0x59 }
        }
    }
    fn read16(&mut self, a: usize) -> u16 {
        if !self.gate() {
            return 0;
        }
        self.i2c.read16(a)
    }
    fn read32(&mut self, a: usize) -> u32 {
        if !self.gate() {
            return 0;
        }
        if p::SNAPSHOTS.contains(&a) {
            let mut v = *self.regs.get(&a).unwrap_or(&0);
            if a == p::RDATA {
                if let Some((command, skip, remaining)) = &mut self.sync_gap {
                    if *command == self.commands.len() && (v >> 16) & 7 == 0 {
                        if *skip > 0 {
                            *skip -= 1;
                        } else if *remaining > 0 {
                            *remaining -= 1;
                            v = self.gap_value;
                        }
                    }
                }
                self.last_rdata = v;
            }
            v
        } else {
            self.i2c.read32(a)
        }
    }
    fn write8(&mut self, a: usize, v: u8) {
        if !self.gate() {
            return;
        }
        assert_eq!(a, BASE);
        assert!(v <= 127);
        self.pointer = v;
        self.i2c.write8(a, v);
    }
    fn write16(&mut self, a: usize, v: u16) {
        if !self.gate() {
            return;
        }
        if a == BASE + 0x24 {
            self.i2c.result = if self.i2c.starts == self.fail_i2c_at {
                self.fail_irq
            } else {
                1
            };
        }
        self.i2c.write16(a, v);
    }
    fn write32(&mut self, a: usize, v: u32) {
        if !self.gate() {
            return;
        }
        if a == p::CMD {
            assert_eq!(
                self.last_rdata & (p::INIT | p::SYNC | (15 << 16)),
                p::INIT | p::SYNC,
                "command must follow an initialized idle/request-clear/SYNC-high sample"
            );
            assert_eq!(v & 0x8000ffff, 0, "read-only even-address command");
            let addr = ((v >> 16) << 1) as u16;
            assert_eq!(addr, p::REGISTERS[self.commands.len()]);
            self.commands.push(addr);
            let value = match addr {
                0x40e => self.read_test,
                8 => 0x5810,
                10 => self.swcid,
                _ => addr ^ 0x1234,
            };
            self.regs.insert(
                p::RDATA,
                p::INIT
                    | p::SYNC
                    | if self.timeout_pmic {
                        2 << 16
                    } else {
                        (6 << 16) | u32::from(value)
                    },
            );
        } else {
            assert_eq!(a, p::CLEAR, "only command and valid-clear writes");
            assert_eq!(v, 1);
            self.clears += 1;
            if !self.ack_stuck {
                self.regs.insert(p::RDATA, p::INIT | p::SYNC);
            }
        }
    }
}

#[test]
fn complete_windows_and_pmic_reads_have_exact_bounds_and_restoration() {
    let mut m = Hardware::ready();
    let r = board6::run(&mut m, &fixture(0));
    assert_eq!(r.words[20], 1);
    assert_eq!(r.words[58], 1);
    assert_eq!(m.i2c.starts, 256);
    assert_eq!(m.commands, p::REGISTERS);
    for (i, e) in r.words[64..1344].chunks_exact(5).enumerate() {
        assert_eq!(e[0], 1);
        assert_eq!(e[3], 1);
        assert_eq!(
            e[1],
            if i % 128 == 0 {
                0x23
            } else {
                (i % 128) as u32 ^ if i < 128 { 0x58 } else { 0x59 }
            }
        );
    }
    assert_eq!(r.words[1344], 1);
    assert_eq!(r.words[1347], 1);
    for e in r.words[1376..1541].chunks_exact(5) {
        assert_eq!(e[0], 1);
        assert_eq!((e[2] >> 16) & 7, 6);
    }
    assert_eq!(m.i2c.regs[&(BASE + 0x20)], 0x10);
    assert_eq!(m.i2c.regs[&(BASE + 0x34)], 3);
    if let Ok(path) = std::env::var("BOARD6_TEST_REPORT") {
        use std::io::Write;
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .unwrap()
            .write_all(&r.encode())
            .unwrap();
    }
}

#[test]
fn partial_window_stops_at_first_error_and_pmic_remains_independent() {
    for at in [0, 1, 127, 128, 255] {
        for irq in [0, 2, 8, 0x201] {
            let mut m = Hardware::ready();
            m.fail_i2c_at = at;
            m.fail_irq = irq;
            let r = board6::run(&mut m, &fixture(0));
            assert_ne!(r.words[20], 1);
            assert_eq!(r.words[58], 1);
            assert_eq!(m.i2c.starts, at + 1);
            assert_eq!(r.words[1344], 1);
            for (i, e) in r.words[64..1344].chunks_exact(5).enumerate() {
                if i < at {
                    assert_eq!(e[0], 1);
                } else if i == at {
                    assert_ne!(e[0], 1);
                    assert_ne!(e[0], 0);
                } else {
                    assert_eq!(e, &[0, u32::MAX, u32::MAX, u32::MAX, u32::MAX]);
                }
            }
            assert!(m.ops < 50_000);
        }
    }
}

fn pmic_only(m: &mut Hardware, blob: &[u8]) -> ([u32; 32], [u32; 165]) {
    let mut h = [0; 32];
    let mut e = [u32::MAX; 165];
    for x in e.chunks_exact_mut(5) {
        x[0] = 0;
    }
    p::run(m, blob, &mut h, &mut e);
    (h, e)
}
#[test]
fn pwrap_inherited_busy_and_invalid_config_never_get_cleared_or_started() {
    for (a, v, status) in [
        (p::INFRA, 2, 4),
        (p::BASE, 1, 5),
        (p::BASE + 4, 0, 5),
        (p::BASE + 0x7c, 1, 5),
        (p::BASE + 0x9c, 0, 5),
        (p::BASE + 0xa0, 0, 5),
        (p::RDATA, p::SYNC, 5),
        (p::RDATA, p::INIT | p::SYNC | (6 << 16), 6),
        (p::RDATA, p::INIT | p::SYNC | (2 << 16), 6),
    ] {
        let mut m = Hardware::ready();
        m.regs.insert(a, v);
        let (h, e) = pmic_only(&mut m, &fixture(0));
        assert_eq!(h[0], status);
        assert!(m.commands.is_empty());
        assert!(e.chunks_exact(5).all(|e| e[0] == 0));
        assert_eq!(m.regs[&a], v);
    }
}

#[test]
fn sync_gaps_before_con14_after_ack_and_at_initial_snapshot_are_bounded() {
    for (commands, skip) in [(0, 0), (0, 1), (31, 2), (31, 1), (33, 1)] {
        let mut m = Hardware::ready();
        m.sync_gap = Some((commands, skip, 7));
        let (h, e) = pmic_only(&mut m, &fixture(0));
        assert_eq!(h[0], 1, "boundary {commands}/{skip}");
        assert_eq!(h[3], 1);
        assert_eq!(m.commands, p::REGISTERS);
        assert_eq!(m.clears, 33);
        assert!(e.chunks_exact(5).all(|e| e[0] == 1));
        assert_eq!(m.sync_gap.unwrap().2, 0);
    }
    // Last permitted pre-command sample succeeds; one more low sample stops.
    for gap in [p::BUDGET as usize - 1, p::BUDGET as usize] {
        let mut m = Hardware::ready();
        m.sync_gap = Some((31, 2, gap));
        let (h, e) = pmic_only(&mut m, &fixture(0));
        if gap < p::BUDGET as usize {
            assert_eq!(h[0], 1);
            assert_eq!(m.commands.len(), 33);
        } else {
            assert_eq!(h[0], 6);
            assert_eq!(m.commands.len(), 31);
            assert_eq!(m.clears, 31);
            assert_eq!(&e[155..160], &[6, u32::MAX, 0x80600010, u32::MAX, u32::MAX]);
            assert_eq!(e[160], 0);
        }
        assert!(m.ops < p::BUDGET as usize + 400);
    }
}

#[test]
fn permanent_sync_low_and_pending_or_lost_init_during_wait_do_not_issue_commands() {
    // A gap in the extra post-ack observation must not be reported as all-done.
    for commands in [31, 33] {
        let mut m = Hardware::ready();
        m.sync_gap = Some((commands, 1, usize::MAX));
        let (h, e) = pmic_only(&mut m, &fixture(0));
        assert_eq!(h[0], 6);
        assert_eq!(h[3], 0);
        assert_eq!(m.commands.len(), commands);
        assert_eq!(m.clears, commands);
        assert_eq!(e[(commands - 1) * 5], 1); // completed read is retained
        assert!(m.ops < p::BUDGET as usize + 400);
    }
    for (value, status) in [
        (0x80600010, 6),
        (p::INIT | p::SYNC | (6 << 16), 6),
        (p::INIT | p::SYNC | (2 << 16), 6),
        (p::INIT | p::SYNC | (1 << 19), 6),
        (p::SYNC, 5),
    ] {
        let mut m = Hardware::ready();
        m.sync_gap = Some((31, 2, usize::MAX));
        m.gap_value = value;
        let (h, e) = pmic_only(&mut m, &fixture(0));
        assert_eq!(h[0], status);
        assert_eq!(h[3], 0);
        assert_eq!(m.commands.len(), 31);
        assert_eq!(m.clears, 31);
        assert_eq!(e[156], u32::MAX);
        assert_eq!(e[160], 0);
        assert!(m.ops < p::BUDGET as usize + 400);
    }
    let mut m = Hardware::ready();
    m.regs.insert(p::RDATA, p::INIT);
    let (h, e) = pmic_only(&mut m, &fixture(0));
    assert_eq!(h[0], 6);
    assert_eq!(h[3], 0);
    assert!(m.commands.is_empty());
    assert_eq!(m.clears, 0);
    assert_eq!(e[0], 6);
    assert_eq!(e[1], u32::MAX);
    assert!(m.ops < p::BUDGET as usize + 20);
}

#[test]
fn faults_in_sync_waits_stop_accesses_without_claiming_idle() {
    let blob = fixture(0);
    for skip in [1, 2] {
        let mut clean = Hardware::ready();
        clean.sync_gap = Some((31, skip, 5));
        pmic_only(&mut clean, &blob);
        for at in 1..=clean.ops {
            let mut m = Hardware::ready();
            m.sync_gap = Some((31, skip, 5));
            m.fault_at = at;
            let (h, _) = pmic_only(&mut m, &blob);
            assert_eq!(h[0], 3);
            assert_eq!(m.ops, at);
            if m.commands.len() >= 31 {
                assert_eq!(h[3], 0);
            }
        }
    }
}
#[test]
fn pmic_timeouts_test_mismatch_and_identity_mismatch_stop_without_fabricated_values() {
    for mode in 0..4 {
        let mut m = Hardware::ready();
        match mode {
            0 => m.timeout_pmic = true,
            1 => m.ack_stuck = true,
            2 => m.read_test = 0,
            3 => m.swcid = 0x6610,
            _ => unreachable!(),
        }
        let (h, e) = pmic_only(&mut m, &fixture(0));
        assert_eq!(h[0], [7, 10, 8, 9][mode]);
        assert_eq!(m.commands.len(), if mode == 3 { 3 } else { 1 });
        if mode == 0 {
            assert_eq!(e[1], u32::MAX);
            assert_eq!(e[3], 32768);
            assert_eq!(h[3], 0);
        }
        if mode == 1 {
            assert_eq!(e[1], 0x5aa5);
            assert_eq!(e[4], 32768);
            assert_eq!(h[3], 0);
        }
        assert!(m.ops < 33_000);
    }
}
#[test]
fn pmic_profile_rejects_wrong_base_soc_compat_and_disabled_parent() {
    let original = fixture(0);
    for change in [1, 7, 8] {
        let mut m = Hardware::ready();
        let (h, _) = pmic_only(&mut m, &fixture(change));
        assert_eq!(h[0], 2);
        assert_eq!(m.ops, 0);
    }
    for (from, to) in [
        (&b"mediatek,MT6768"[..], &b"mediatek,MT6765"[..]),
        (&b"mt6768-pwrap"[..], &b"mt6765-pwrap"[..]),
        (b"mediatek,mt6358", b"mediatek,mt6359"),
        (&[0x10, 0, 0xd0, 0], &[0x10, 0, 0xc0, 0]),
    ] {
        let mut blob = original.clone();
        let pos = blob.windows(from.len()).position(|x| x == from).unwrap();
        blob[pos..pos + from.len()].copy_from_slice(to);
        let mut m = Hardware::ready();
        let (h, _) = pmic_only(&mut m, &blob);
        assert_eq!(h[0], 2);
        assert_eq!(m.ops, 0);
    }
    for len in [0, 39, 40, original.len() - 1] {
        let mut m = Hardware::ready();
        let (h, _) = pmic_only(&mut m, &original[..len]);
        assert_eq!(h[0], 2);
        assert_eq!(m.ops, 0);
    }
}
#[test]
fn every_mmio_fault_stops_later_hardware_access_and_never_claims_complete() {
    let blob = fixture(0);
    let mut m = Hardware::ready();
    board6::run(&mut m, &blob);
    for at in 1..=m.ops {
        let mut h = Hardware::ready();
        h.fault_at = at;
        let r = board6::run(&mut h, &blob);
        assert_eq!(h.ops, at);
        assert_eq!(r.words[1344], 3, "fault {at}");
    }
}
#[test]
fn report_roundtrip_handles_final_short_chunk_and_configuration_reset() {
    let blob = fixture(0);
    let bytes = board6::run(&mut Hardware::ready(), &blob).encode();
    let mut c = Control::new(&blob);
    c.set_report(&bytes);
    let mut out = [0; 64];
    assert_eq!(usb_control::DEVICE[12], 0x46);
    assert_eq!(c.setup(setup(0xc0, 0xb2, 0, 64), &mut out), Reply::Stall);
    c.setup(setup(0, 5, 7, 0), &mut out);
    c.status_complete();
    c.setup(setup(0, 9, 1, 0), &mut out);
    c.status_complete();
    let mut all = vec![];
    for number in 0..134 {
        assert_eq!(
            c.setup(setup(0xc0, 0xb3, number, 64), &mut out),
            Reply::Data(64)
        );
        assert_eq!(&out[..4], b"A6CH");
        let n = u16::from_le_bytes(out[8..10].try_into().unwrap()) as usize;
        assert_eq!(n, if number == 133 { 16 } else { 48 });
        assert_eq!(
            u32::from_le_bytes(out[4..8].try_into().unwrap()),
            u32::from(number) * 48
        );
        assert_eq!(
            u32::from_le_bytes(out[12..16].try_into().unwrap()),
            board_pages::crc32(&out[16..16 + n])
        );
        assert!(out[16 + n..].iter().all(|x| *x == 0));
        all.extend_from_slice(&out[16..16 + n]);
    }
    assert_eq!(all, bytes);
    for s in [
        setup(0x40, 0xb3, 0, 64),
        setup(0xc0, 0xb3, 134, 64),
        setup(0xc0, 0xb2, 1, 64),
        setup(0xc0, 0xb2, 0, 63),
    ] {
        assert_eq!(c.setup(s, &mut out), Reply::Stall);
    }
    assert_eq!(c.setup(setup(0xc0, 0xb2, 0, 64), &mut out), Reply::Data(64));
    assert_eq!(&out[..4], b"A6IF");
    let info = out;
    c.reset();
    assert_eq!(c.setup(setup(0xc0, 0xb2, 0, 64), &mut out), Reply::Stall);
    c.setup(setup(0, 5, 7, 0), &mut out);
    c.status_complete();
    c.setup(setup(0, 9, 1, 0), &mut out);
    c.status_complete();
    c.setup(setup(0xc0, 0xb2, 0, 64), &mut out);
    assert_eq!(out, info);
}
