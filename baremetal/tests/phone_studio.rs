#![allow(dead_code)]
#[path = "../src/board_screen.rs"]
mod board_screen;
#[path = "../src/dashboard.rs"]
mod dashboard;
#[path = "../src/device_tree.rs"]
mod device_tree;
#[path = "../src/phone_studio.rs"]
mod phone_studio;
#[path = "../src/tone.rs"]
mod tone;

use phone_studio::{Checkpoint, View};

const USB: phone_studio::UsbSnapshot = phone_studio::UsbSnapshot {
    power: u8::MAX,
    devctl: u8::MAX,
    hwvers: u16::MAX,
    csr0: u16::MAX,
    acr6: u32::MAX,
    dtm0: u32::MAX,
    dtm1: u32::MAX,
    infra: u32::MAX,
    top: u32::MAX,
};

#[test]
fn preconnect_register_labels_fit_without_truncation() {
    for line in [USB.controller_line(), USB.phy_line()] {
        assert!(line.text().len() <= 76);
        assert!(!line.text().contains('~'));
        assert!(20 + line.text().len() * 9 < dashboard::WIDTH);
    }
}

pub fn sample(tone: &tone::Report) -> View<'_> {
    View {
        revision: "0123456789abcdef0123456789abcdef01234567",
        dirty: false,
        passed: 7,
        failed: 0,
        faults: 0,
        i2c_status: 1,
        amp_reads: 256,
        i2c_restored: true,
        pmic_status: 1,
        pmic_reads: 33,
        pmic_idle: true,
        report_crc: 0x12345678,
        usb: &USB,
        tone,
    }
}

pub fn complete() -> tone::Report {
    let mut t = tone::Report::new();
    t.outcome = 1;
    t.off = tone::OFF_MASK;
    t.restored = true;
    t.frequency = tone::HZ;
    t.start = 1;
    t.opened = 2;
    t.closed = 3_250_002;
    t.end = 3_250_003;
    t
}

#[test]
fn only_complete_register_evidence_gets_the_green_checkpoint() {
    let t = complete();
    assert_eq!(sample(&t).checkpoint(), Checkpoint::RegistersPass);
    for bit in [1, 2, 4] {
        let mut v = sample(&t);
        v.passed &= !bit;
        assert_eq!(v.checkpoint(), Checkpoint::Review);
        v.passed = 7;
        v.failed = bit;
        assert_eq!(v.checkpoint(), Checkpoint::Review);
    }
    let bad: &[fn(&mut View<'_>)] = &[
        |v| v.dirty = true,
        |v| v.revision = "unknown",
        |v| v.faults = 1,
        |v| v.i2c_status = 2,
        |v| v.amp_reads = 255,
        |v| v.i2c_restored = false,
        |v| v.pmic_status = 2,
        |v| v.pmic_reads = 32,
        |v| v.pmic_idle = false,
        |v| v.passed = 15,
    ];
    for change in bad {
        let mut v = sample(&t);
        change(&mut v);
        assert_eq!(v.checkpoint(), Checkpoint::Review);
    }
}

#[test]
fn shutdown_uncertainty_has_priority_over_other_results() {
    for outcome in [0, 1, 3, 4, u32::MAX] {
        for bit in 0..17 {
            let mut t = complete();
            t.outcome = outcome;
            t.off &= !(1 << bit);
            assert_eq!(sample(&t).checkpoint(), Checkpoint::OffUnverified);
        }
    }
}

#[test]
fn preflight_rejection_does_not_claim_audio_was_activated_or_off() {
    let mut t = tone::Report::new();
    t.outcome = 2;
    t.error = 1;
    assert_eq!(sample(&t).checkpoint(), Checkpoint::Review);
    assert!(!t.verified_off());
    assert_eq!(phone_studio::outcome_name(t.outcome), "Preflight rejected");
}

#[test]
fn incomplete_cleanup_and_restore_never_claim_complete() {
    for field in 0..4 {
        let mut t = complete();
        match field {
            0 => t.error = 10,
            1 => t.cleanup_errors = 1,
            2 => t.restored = false,
            _ => t.outcome = 3,
        }
        assert_eq!(sample(&t).checkpoint(), Checkpoint::Review);
    }
    for error in 0..=12 {
        assert_ne!(phone_studio::error_name(error), "unknown");
    }
    assert_eq!(phone_studio::error_name(13), "unknown");
}

#[test]
fn inconsistent_timing_is_not_a_green_checkpoint() {
    for field in 0..6 {
        let mut t = complete();
        match field {
            0 => t.frequency = 0,
            1 => t.start = 0,
            2 => t.start = t.opened + 1,
            3 => t.opened = t.start + 2 * tone::HZ,
            4 => t.closed = t.opened + 3_249_999,
            _ => t.end = t.closed - 1,
        }
        assert_eq!(sample(&t).checkpoint(), Checkpoint::Review);
    }
}

#[test]
fn fatal_presentation_removes_all_stale_checkpoint_pixels() {
    struct Frame(Vec<u32>);
    impl dashboard::Canvas for Frame {
        fn pixel(&mut self, x: usize, y: usize, color: u32) {
            self.0[y * dashboard::WIDTH + x] = color;
        }
    }
    let t = complete();
    let mut frame = Frame(vec![0; dashboard::WIDTH * dashboard::HEIGHT]);
    phone_studio::paint(&mut frame, solid, &sample(&t));
    assert!(frame.0.contains(&dashboard::MINT));
    phone_studio::clear_for_fatal(&mut frame);
    assert!(frame.0.iter().all(|pixel| *pixel == dashboard::INK));
    dashboard::text(
        &mut frame,
        solid,
        20,
        68,
        1,
        phone_studio::FATAL_BANNER,
        dashboard::GOLD,
    );
    assert!(frame.0.contains(&dashboard::GOLD));
    assert!(frame.0[100 * dashboard::WIDTH..]
        .iter()
        .all(|pixel| *pixel == dashboard::INK));
    assert!(phone_studio::FATAL_BANNER.contains("AUDIO-TONE1 / FATAL"));
}

struct Bounds;
impl dashboard::Canvas for Bounds {
    fn pixel(&mut self, x: usize, y: usize, color: u32) {
        assert!(x < dashboard::WIDTH && y < dashboard::HEIGHT, "{x},{y}");
        if color != dashboard::INK {
            assert!(
                (60..1620).contains(&y),
                "content in cutout/bottom guard: {y}"
            );
        }
    }
}
fn solid(_: u8) -> [u8; 16] {
    [255; 16]
}

#[test]
fn red_alarm_renders_and_live_cell_geometry_is_isolated() {
    struct Frame(Vec<u32>);
    impl dashboard::Canvas for Frame {
        fn pixel(&mut self, x: usize, y: usize, color: u32) {
            self.0[y * dashboard::WIDTH + x] = color;
        }
    }
    let mut t = complete();
    t.outcome = 4;
    t.off &= !1;
    let mut v = sample(&t);
    v.dirty = true;
    v.failed = 7;
    let mut frame = Frame(vec![0; dashboard::WIDTH * dashboard::HEIGHT]);
    phone_studio::paint(&mut frame, solid, &v);
    assert_eq!(frame.0[346 * dashboard::WIDTH + 60], 0xffff9d9d);
    assert_eq!(frame.0[1154 * dashboard::WIDTH + 60], 0xffff9d9d);
    let original = frame.0.clone();
    // Same 9x16 opaque-cell geometry as the existing production status updater;
    // this is a composition check, not a simulation of USB polling latency.
    for column in 0..76 {
        dashboard::rect(
            &mut frame,
            20 + column * 9,
            phone_studio::USB_STATUS_Y,
            9,
            16,
            dashboard::INK,
        );
        dashboard::text(
            &mut frame,
            solid,
            20 + column * 9,
            phone_studio::USB_STATUS_Y,
            1,
            "X",
            dashboard::MINT,
        );
    }
    for (index, (old, new)) in original.iter().zip(&frame.0).enumerate() {
        let x = index % dashboard::WIDTH;
        let y = index / dashboard::WIDTH;
        if !(20..704).contains(&x)
            || !(phone_studio::USB_STATUS_Y..phone_studio::USB_STATUS_Y + 16).contains(&y)
        {
            assert_eq!(old, new, "live strip changed outside pixel {x},{y}");
        }
    }
}

#[test]
fn every_state_and_extreme_counter_fits_the_framebuffer() {
    for outcome in [0, 1, 2, 3, 4, u32::MAX] {
        for error in 0..=13 {
            let mut t = complete();
            t.outcome = outcome;
            t.error = error;
            if outcome == 4 {
                t.off &= !1;
            }
            phone_studio::paint(&mut Bounds, solid, &sample(&t));
        }
    }
    let mut t = complete();
    t.cleanup_errors = u32::MAX;
    let mut v = sample(&t);
    v.faults = u64::MAX;
    v.amp_reads = usize::MAX;
    v.pmic_reads = usize::MAX;
    v.i2c_status = u32::MAX;
    v.pmic_status = u32::MAX;
    phone_studio::paint(&mut Bounds, solid, &v);
    // All 76 cells of the existing live updater have an unobstructed strip.
    assert!(20 + 76 * 9 < dashboard::WIDTH);
    assert!(phone_studio::USB_STATUS_Y + 16 < 1480);
}
