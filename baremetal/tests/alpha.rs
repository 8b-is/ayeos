#![allow(dead_code)]
extern crate alloc;
// Original, byte-identical upstream reference, compiled ONLY by this host test.
#[path = "../src/board_pages.rs"]
mod board_pages;
#[path = "../src/board_screen.rs"]
mod board_screen;
#[path = "../src/device_tree.rs"]
mod device_tree;
#[path = "../src/diagnostics.rs"]
mod diagnostics;
#[path = "../vendor/marine-core/reference/internal_math.rs"]
mod internal_math;
#[path = "../src/marine_fixture.rs"]
mod marine_fixture;
#[path = "../vendor/marine-core/reference/marine_int.rs"]
mod marine_int;
#[path = "../vendor/marine-core/reference/rational.rs"]
mod rational;
#[path = "../src/usb_control.rs"]
mod usb_control;
use diagnostics::*;

#[test]
fn marine_matches_unmodified_upstream_per_sample_and_stats() {
    for mode in 0..6 {
        let mut reference = marine_int::MarineInt::new();
        let mut adapted = marine_core::Marine::new();
        for i in 0..marine_core::MAX_SAMPLES {
            let sample = match mode {
                0 => 0,
                1 => marine_fixture::sample(i % marine_fixture::SAMPLES),
                2 => {
                    if i & 1 == 0 {
                        i16::MIN
                    } else {
                        i16::MAX
                    }
                }
                3 => {
                    if i < 512 || i >= 1536 {
                        marine_fixture::sample(128 + i % 384)
                    } else {
                        0
                    }
                }
                4 => {
                    if i % 89 == 0 {
                        30000
                    } else {
                        1
                    }
                }
                _ => {
                    ((i as u32).wrapping_mul(1664525).wrapping_add(1013904223) >> 16) as u16 as i16
                }
            };
            let a = adapted.process(sample).unwrap();
            let r = reference.process_sample(i32::from(sample));
            assert_eq!((a.num, a.den), (r.num, r.den), "mode {mode} sample {i}");
            let (a, r) = (adapted.stats(), reference.stats());
            assert_eq!(a.sample_index, r.sample_index);
            assert_eq!(a.peak_count, r.peak_count);
            assert_eq!(
                (a.avg_period.num, a.avg_period.den),
                (r.avg_period.num, r.avg_period.den)
            );
            assert_eq!(
                (a.avg_amplitude.num, a.avg_amplitude.den),
                (r.avg_amplitude.num, r.avg_amplitude.den)
            );
            assert!(
                a.current_salience.num >= 0
                    && a.current_salience.num as u32 <= a.current_salience.den
            );
        }
        assert!(adapted.process(1234).is_none());
        assert_eq!(adapted.stats().sample_index, marine_core::MAX_SAMPLES);
        adapted.reset();
        reference.reset();
        for sample in [0, 30000, -30000, 1000, 0] {
            let a = adapted.process(sample).unwrap();
            let r = reference.process_sample(i32::from(sample));
            assert_eq!((a.num, a.den), (r.num, r.den));
        }
    }
    assert!(core::mem::size_of::<marine_core::Marine>() <= 256);
}

#[test]
fn fixture_reference_hash() {
    let mut reference = marine_int::MarineInt::new();
    let mut hash = marine_fixture::FNV_OFFSET;
    for i in 0..marine_fixture::SAMPLES {
        let r = reference.process_sample(i32::from(marine_fixture::sample(i)));
        hash = marine_fixture::hash_bytes(hash, &r.num.to_le_bytes());
        hash = marine_fixture::hash_bytes(hash, &r.den.to_le_bytes());
    }
    println!("Marine upstream fixture FNV64={hash:016x}");
    assert_eq!(hash, marine_fixture::REFERENCE_HASH);
    assert_eq!(marine_fixture::run(), (hash, marine_fixture::SAMPLES));
}

#[test]
fn recovery_is_exact_site_and_synchronous_same_el_only() {
    let sites = [Fixup {
        pc: 0x4000,
        resume: 0x4004,
    }];
    let test = Fixup {
        pc: 0x8000,
        resume: 0x8004,
    };
    let mut f = Fault {
        vector: 4,
        level: 1,
        esr: (0x25 << 26) | (1 << 25) | 0x10,
        pc: 0x4000,
        ..Fault::ZERO
    };
    assert_eq!(recovery(&f, &sites, test, false), Some((0x4004, 2)));
    for vector in 0..16 {
        f.vector = vector;
        assert_eq!(
            recovery(&f, &sites, test, true).is_some(),
            matches!(vector, 0 | 4)
        );
    }
    f.vector = 4;
    for ec in 0..64 {
        f.esr = (ec << 26) | (1 << 25);
        assert_eq!(recovery(&f, &sites, test, true).is_some(), ec == 0x25);
    }
    f.esr = (0x25 << 26) | (1 << 25);
    for level in [0, 3, u64::MAX] {
        f.level = level;
        assert!(recovery(&f, &sites, test, true).is_none());
    }
    f.level = 2;
    f.pc += 4;
    assert!(recovery(&f, &sites, test, true).is_none());
    f.pc = test.pc;
    f.esr = (0x3c << 26) | (1 << 25) | 0xa8;
    assert!(recovery(&f, &sites, test, false).is_none());
    assert_eq!(recovery(&f, &sites, test, true), Some((0x8004, 1)));
    f.esr ^= 1;
    assert!(recovery(&f, &sites, test, true).is_none());
    f.esr ^= 1 | (1 << 25);
    assert!(recovery(&f, &sites, test, true).is_none());
    assert_eq!(core::mem::size_of::<Fault>(), 64);
}

fn snapshot(journal: &Journal) -> [u8; BYTES] {
    encode(Snapshot {
        revision: "0123456789012345678901234567890123456789",
        dirty: true,
        passed: 7,
        failed: 0,
        board_crc: 0x12345678,
        fault_count: 0,
        fault: Fault::ZERO,
        marine_hash: 123,
        marine_samples: 1024,
        journal,
    })
}
fn u64_at(b: &[u8], n: usize) -> u64 {
    u64::from_le_bytes(b[n..n + 8].try_into().unwrap())
}
#[test]
fn journal_wraps_in_order_and_saturates_sequence() {
    let mut j = Journal::new();
    for i in 0..41 {
        j.record(2, 3, i);
    }
    let bytes = snapshot(&j);
    assert_eq!(u64_at(&bytes, 32), 25);
    for i in 0..16 {
        assert_eq!(u64_at(&bytes, 256 + i * 32), 26 + i as u64);
        assert_eq!(u64_at(&bytes, 280 + i * 32), 25 + i as u64);
    }
    assert!(bytes[768..].iter().all(|b| *b == 0));
    j.total = u64::MAX - 1;
    j.record(7, 8, 9);
    let before = snapshot(&j);
    j.record(10, 11, 12);
    assert_eq!(snapshot(&j), before);
}

#[test]
fn rust_to_python_wire_fixture() {
    let mut journal = Journal::new();
    for i in 0..20 {
        journal.record(i % 7 + 1, 1, i);
    }
    let bytes = encode(Snapshot {
        revision: "0123456789012345678901234567890123456789",
        dirty: false,
        passed: 7,
        failed: 0,
        board_crc: 0x12345678,
        fault_count: 0,
        fault: Fault {
            vector: 4,
            level: 1,
            esr: (0x3c << 26) | (1 << 25) | 0xa8,
            pc: 0x40080000,
            sp: 0x40100000,
            spsr: 5,
            action: 1,
            ..Fault::ZERO
        },
        marine_hash: marine_fixture::REFERENCE_HASH,
        marine_samples: 1024,
        journal: &journal,
    });
    if let Ok(path) = std::env::var("AYEOS_ALPHA_TEST_DUMP") {
        std::fs::write(path, bytes).unwrap();
    }
}

#[test]
fn usb_snapshot_is_frozen_bounded_and_reset_stable() {
    use usb_control::{Control, Reply};
    let data = snapshot(&Journal::new());
    let mut c = Control::new(b"fdt");
    let mut out = [0; 64];
    c.set_diagnostics(&data);
    assert_eq!(
        c.setup([0xc0, 0xb4, 0, 0, 0, 0, 64, 0], &mut out),
        Reply::Stall
    );
    c.configuration = 1;
    assert_eq!(
        c.setup([0xc0, 0xb4, 0, 0, 0, 0, 64, 0], &mut out),
        Reply::Data(64)
    );
    assert_eq!(&out[..4], b"ADIF");
    let info = out;
    let mut received = Vec::new();
    for n in 0..22 {
        assert_eq!(
            c.setup([0xc0, 0xb5, n, 0, 0, 0, 64, 0], &mut out),
            Reply::Data(64)
        );
        assert_eq!(&out[..4], b"ADCH");
        let len = u16::from_le_bytes([out[8], out[9]]) as usize;
        assert_eq!(len, (1024 - n as usize * 48).min(48));
        assert!(out[16 + len..].iter().all(|b| *b == 0));
        received.extend_from_slice(&out[16..16 + len]);
    }
    assert_eq!(received, data);
    for setup in [
        [0xc0, 0xb5, 22, 0, 0, 0, 64, 0],
        [0xc0, 0xb5, 255, 255, 255, 255, 64, 0],
        [0xc0, 0xb4, 1, 0, 0, 0, 64, 0],
        [0xc0, 0xb4, 0, 0, 1, 0, 64, 0],
        [0xc0, 0xb4, 0, 0, 0, 0, 63, 0],
        [0x40, 0xb4, 0, 0, 0, 0, 64, 0],
    ] {
        assert_eq!(c.setup(setup, &mut out), Reply::Stall);
    }
    c.reset();
    c.configuration = 1;
    c.setup([0xc0, 0xb4, 0, 0, 0, 0, 64, 0], &mut out);
    assert_eq!(out, info);
    assert_eq!(usb_control::DEVICE[12..14], [0x61, 0x05]);
}
