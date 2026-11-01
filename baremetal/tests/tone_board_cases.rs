use super::*;
struct Tree {
    s: Vec<u8>,
    names: Vec<u8>,
}
impl Tree {
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
        self.pad();
    }
    fn pad(&mut self) {
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
        self.pad();
    }
    fn finish(mut self) -> Vec<u8> {
        self.word(9);
        let mut b = vec![0; 56];
        for (i, v) in [
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
            b[i * 4..i * 4 + 4].copy_from_slice(&(v as u32).to_be_bytes());
        }
        b.extend(self.s);
        b.extend(self.names);
        b
    }
}
fn fixture(change: u8) -> Vec<u8> {
    let mut f = Tree::new();
    f.begin("");
    for _ in 0..if change == 1 { 2 } else { 1 } {
        f.begin("timer");
        f.prop("compatible", b"arm,armv8-timer\0");
        f.prop(
            "clock-frequency",
            &(if change == 2 {
                26_000_000u32
            } else {
                13_000_000
            })
            .to_be_bytes(),
        );
        f.end();
    }
    f.begin("pwrap@1000d000");
    f.begin("mt6358-pmic");
    if change == 3 {
        f.prop("status", b"disabled\0");
    }
    f.begin("mt6358regulator");
    if change == 4 {
        f.prop("ranges", b"");
    }
    f.begin("ldo_vaud28");
    f.prop("regulator-name", b"vaud28\0");
    f.prop("regulator-min-microvolt", &2_800_000u32.to_be_bytes());
    f.prop(
        "regulator-max-microvolt",
        &(if change == 5 { 2_700_000u32 } else { 2_800_000 }).to_be_bytes(),
    );
    if change != 6 {
        f.prop("regulator-always-on", b"");
    }
    f.end();
    f.end();
    f.begin("mt6358codec");
    f.prop(
        "compatible",
        if change == 7 {
            b"other\0"
        } else {
            b"mediatek,mt6358-sound\0"
        },
    );
    f.end();
    f.end();
    f.end();
    f.end();
    f.finish()
}
#[test]
fn supply_timer_and_ancestry_are_exact_and_unambiguous() {
    assert_eq!(tone_board::matches(&fixture(0)), Ok(true));
    for change in 1..=7 {
        assert_eq!(
            tone_board::matches(&fixture(change)),
            Ok(false),
            "change={change}"
        );
    }
    let f = fixture(0);
    for n in 0..f.len() {
        assert!(tone_board::matches(&f[..n]).is_err());
    }
}
#[test]
#[ignore = "explicit private FDT path only; no private bytes in repository"]
fn private_phone_fdt_matches_all_three_profiles() {
    let p = std::env::var("AYEOS_PRIVATE_FDT").expect("explicit path required");
    let b = std::fs::read(p).unwrap();
    assert_eq!(audio_board::matches(&b), Ok(true));
    assert_eq!(pmic_probe::matches(&b), Ok(true));
    assert_eq!(tone_board::matches(&b), Ok(true));
}
