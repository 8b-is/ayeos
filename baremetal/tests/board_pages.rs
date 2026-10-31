#[path = "../src/board_pages.rs"]
mod board_pages;
#[path = "../src/board_screen.rs"]
mod board_screen;
#[path = "../src/device_tree.rs"]
mod device_tree;

use board_pages::{crc32, Inventory, PageKey, PAGE_ROWS};
use std::collections::BTreeMap;

struct Fdt {
    structure: Vec<u8>,
    strings: Vec<u8>,
}
impl Fdt {
    fn new() -> Self {
        let mut s = Self {
            structure: Vec::new(),
            strings: Vec::new(),
        };
        s.begin("");
        s
    }
    fn word(&mut self, word: u32) {
        self.structure.extend(word.to_be_bytes());
    }
    fn pad(&mut self) {
        while self.structure.len() % 4 != 0 {
            self.structure.push(0);
        }
    }
    fn begin(&mut self, name: &str) {
        self.word(1);
        self.structure.extend(name.as_bytes());
        self.structure.push(0);
        self.pad();
    }
    fn end(&mut self) {
        self.word(2);
    }
    fn prop(&mut self, name: &str, value: &[u8]) {
        self.word(3);
        self.word(value.len() as u32);
        self.word(self.strings.len() as u32);
        self.strings.extend(name.as_bytes());
        self.strings.push(0);
        self.structure.extend(value);
        self.pad();
    }
    fn cells(&mut self, name: &str, words: &[u32]) {
        self.prop(
            name,
            &words
                .iter()
                .flat_map(|w| w.to_be_bytes())
                .collect::<Vec<_>>(),
        );
    }
    fn provider(&mut self, name: &str, handle: u32, cells: u32) {
        self.begin(name);
        self.cells("#clock-cells", &[cells]);
        self.cells("clock-frequency", &[26_000_000]);
        self.cells("phandle", &[handle]);
        self.end();
    }
    fn finish(mut self) -> Vec<u8> {
        self.end();
        self.word(9);
        let mut blob = vec![0; 56];
        blob.extend(&self.structure);
        blob.extend(&self.strings);
        let header = [
            0xd00dfeed,
            blob.len() as u32,
            56,
            (56 + self.structure.len()) as u32,
            40,
            17,
            16,
            0,
            self.strings.len() as u32,
            self.structure.len() as u32,
        ];
        for (i, word) in header.iter().enumerate() {
            blob[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
        }
        blob
    }
}

fn all_pages(blob: &[u8], inventory: &Inventory) -> Vec<String> {
    let (pages, capped) = inventory.page_count(blob);
    let mut result = Vec::new();
    for page in 0..pages {
        let mut lines = Vec::new();
        let stats = inventory.render(blob, page, |s| lines.push(s.to_owned()));
        assert_eq!(stats.capped, capped);
        assert!(lines.len() <= PAGE_ROWS);
        assert!(lines
            .iter()
            .all(|s| s.len() <= 76 && s.is_ascii() && !s.contains('~')));
        result.extend(lines);
    }
    assert_eq!(
        result.len(),
        inventory.render(blob, usize::MAX, |_| {}).lines
    );
    result
}

#[derive(Default)]
struct Record {
    name: Vec<u8>,
    data: Vec<u8>,
    bytes: Option<usize>,
    crc: Option<u32>,
}
fn records(lines: &[String]) -> BTreeMap<String, Record> {
    let mut out = BTreeMap::<String, Record>::new();
    for line in lines {
        if !line.starts_with('R') {
            continue;
        }
        let (id, rest) = line.split_once(' ').unwrap();
        let r = out.entry(id.to_owned()).or_default();
        if let Some(text) = rest.strip_prefix("NAME+") {
            let (offset, value) = text.split_once(' ').unwrap();
            assert_eq!(r.name.len(), usize::from_str_radix(offset, 16).unwrap());
            r.name.extend(value.as_bytes());
        } else if let Some(text) = rest.strip_prefix("BYTES=") {
            let (bytes, crc) = text.split_once(" CRC32=").unwrap();
            r.bytes = Some(bytes.parse().unwrap());
            r.crc = Some(u32::from_str_radix(crc, 16).unwrap());
        } else if let Some(text) = rest.strip_prefix('+') {
            let (offset, hex) = text.split_once(' ').unwrap();
            assert_eq!(r.data.len(), usize::from_str_radix(offset, 16).unwrap());
            assert_eq!(hex.len() % 2, 0);
            for bytes in hex.as_bytes().chunks(2) {
                r.data
                    .push(u8::from_str_radix(std::str::from_utf8(bytes).unwrap(), 16).unwrap());
            }
        }
    }
    for r in out.values() {
        assert_eq!(r.bytes, Some(r.data.len()));
        assert_eq!(r.crc, Some(crc32(&r.data)));
    }
    out
}

#[test]
fn full_values_cross_pages_without_loss_and_private_properties_are_excluded() {
    let mut f = Fdt::new();
    f.begin("chosen");
    f.prop("bootargs", b"PRIVATE_CREDENTIALS");
    f.end();
    f.begin("pwrap@1000d000");
    f.begin("mt6358-pmic");
    for _ in 0..40 {
        f.begin("rtc");
        f.prop("compatible", b"mediatek,mt6358-rtc\0");
        f.end();
    }
    f.end();
    f.end();
    f.begin("consys@18002000");
    let reg: Vec<u8> = (0u8..160).collect();
    let interrupts: Vec<u8> = (100u8..148).collect();
    f.prop("reg", &reg);
    f.prop("interrupts", &interrupts);
    f.prop("reg-names", b"mcu\0rgu\0spm\0topckgen\0");
    f.prop("password", b"PRIVATE_CREDENTIALS");
    for _ in 0..28 {
        f.cells("clock-frequency", &[26_000_000]);
    }
    f.end();
    // Long paths must be reconstructable too.
    let name = format!("audio{}", "x".repeat(200));
    f.begin(&name);
    f.prop("status", b"okay\0");
    f.end();
    let blob = f.finish();
    let inv = Inventory::build(&blob).unwrap();
    let lines = all_pages(&blob, &inv);
    assert!(inv.page_count(&blob).0 > 1);
    assert!(!inv.page_count(&blob).1);
    let recs = records(&lines);
    assert!(recs.values().any(|r| r.name == b"reg" && r.data == reg));
    assert!(recs
        .values()
        .any(|r| r.name == b"interrupts" && r.data == interrupts));
    assert!(!recs
        .values()
        .any(|r| r.name == b"bootargs" || r.name == b"password"));
    assert!(!recs
        .values()
        .any(|r| r.data.windows(7).any(|s| s == b"PRIVATE")));
    let mut paths = Vec::new();
    let mut path: Vec<u8> = Vec::new();
    for line in &lines {
        if line.starts_with("NODE ") {
            if !path.is_empty() {
                paths.push(std::mem::take(&mut path));
            }
        }
        if let Some(rest) = line.strip_prefix("PATH+") {
            let (offset, text) = rest.split_once(' ').unwrap();
            assert_eq!(path.len(), usize::from_str_radix(offset, 16).unwrap());
            path.extend(text.as_bytes());
        }
    }
    paths.push(path);
    assert!(paths.iter().any(|p| *p == format!("/{name}/").as_bytes()));
    assert!(!lines.join("\n").contains("mt6358-rtc"));
}

#[test]
fn clock_arguments_are_not_phandles_and_zero_cell_references_repeat() {
    let mut f = Fdt::new();
    f.provider("clock-zero", 0x12, 0);
    f.provider("clock-one", 0x6e, 1);
    f.provider("clock-two", 0x55, 2);
    f.provider("not-a-reference", 0x99, 0);
    f.begin("audio@11220000");
    f.cells("clocks", &[0x12, 0x12, 0x6e, 0x99, 0x55, 0x99, 0x99]);
    f.end();
    let blob = f.finish();
    let inv = Inventory::build(&blob).unwrap();
    assert_eq!(inv.unresolved, 0);
    assert_eq!(inv.malformed_refs, 0);
    let text = all_pages(&blob, &inv).join("\n");
    for name in ["clock-zero/", "clock-one/", "clock-two/"] {
        assert!(text.contains(name));
    }
    assert!(!text.contains("not-a-reference"));
}

#[test]
fn unknown_clock_stops_decoding_and_missing_or_truncated_cells_are_reported() {
    let mut f = Fdt::new();
    f.provider("must-not-be-guessed", 0x12, 0);
    f.begin("audio");
    f.cells("clocks", &[0x999, 0x12]);
    f.end();
    let blob = f.finish();
    let inv = Inventory::build(&blob).unwrap();
    assert_eq!(inv.unresolved, 1);
    assert!(!all_pages(&blob, &inv)
        .join("\n")
        .contains("must-not-be-guessed"));
    for count in [1, u32::MAX] {
        let mut f = Fdt::new();
        f.provider("clk", 0x12, count);
        f.begin("audio");
        f.cells("clocks", &[0x12]);
        f.end();
        assert_eq!(Inventory::build(&f.finish()).unwrap().malformed_refs, 1);
    }
}

#[test]
fn amplifier_parent_and_referenced_pin_children_are_included() {
    let mut f = Fdt::new();
    f.begin("pinctrl");
    f.begin("amp-pins");
    f.cells("phandle", &[7]);
    f.begin("pins1");
    f.cells("pinmux", &[0x1234]);
    f.prop("bias-disable", &[]);
    f.end();
    f.end();
    f.end();
    f.begin("i2c@11004000");
    f.cells("reg", &[0, 0x11004000, 0, 0x1000]);
    f.begin("chip@58");
    f.cells("reg", &[0x58]);
    f.prop("compatible", b"awinic,aw87xxx_pa\0");
    f.cells("pinctrl-0", &[7]);
    f.end();
    f.end();
    let blob = f.finish();
    let inv = Inventory::build(&blob).unwrap();
    assert_eq!(inv.unresolved, 0);
    let lines = all_pages(&blob, &inv);
    let text = lines.join("\n");
    assert!(text.contains("/i2c@11004000/"));
    assert!(text.contains("/pinctrl/amp-pins/pins1/"));
    let recs = records(&lines);
    assert!(recs
        .values()
        .any(|r| r.name == b"pinmux" && r.data == [0, 0, 0x12, 0x34]));
    assert!(recs
        .values()
        .any(|r| r.name == b"bias-disable" && r.data.is_empty()));
}

#[test]
fn capacity_and_duplicate_handle_failures_are_visible() {
    let mut f = Fdt::new();
    for i in 0..140 {
        f.begin(&format!("audio{i}"));
        f.cells("reg", &[i]);
        f.end();
    }
    let inv = Inventory::build(&f.finish()).unwrap();
    assert!(inv.node_overflow > 0);
    let mut f = Fdt::new();
    for i in 1..=513 {
        f.provider(&format!("clk{i}"), i, 0);
    }
    assert_eq!(Inventory::build(&f.finish()).unwrap().provider_overflow, 1);
    let mut f = Fdt::new();
    f.provider("first", 5, 0);
    f.provider("second", 5, 0);
    f.begin("audio");
    f.cells("clocks", &[5]);
    f.end();
    let inv = Inventory::build(&f.finish()).unwrap();
    assert_eq!(inv.duplicate_handles, 1);
    assert_eq!(inv.unresolved, 1);
    let mut f = Fdt::new();
    f.begin("consys");
    f.prop("reg", &vec![0x5a; 100_000]);
    f.end();
    let blob = f.finish();
    let inv = Inventory::build(&blob).unwrap();
    assert!(inv.page_count(&blob).1);
    assert_eq!(inv.render(&blob, 0, |_| {}).lines, board_pages::MAX_LINES);
}

#[test]
fn crc_vectors_and_invalid_fdt() {
    assert_eq!(crc32(b"123456789"), 0xcbf43926);
    assert_eq!(crc32(b""), 0);
    assert!(Inventory::build(&[0; 40]).is_err());
}

#[test]
fn page_key_debounces_and_requires_release_without_hold_repeat() {
    let mut key = PageKey::new(0xffff);
    for sample in [0xfffe, 0xffff, 0xfffe, 0xffff] {
        assert!(!key.sample(sample));
    }
    for _ in 0..100 {
        assert!(!key.sample(0xfffe));
    }
    assert!(!key.sample(0xffff));
    assert!(!key.sample(0xffff));
    assert!(key.sample(0xffff));
    for _ in 0..100 {
        assert!(!key.sample(0xffff));
    }
    // Zero is the inherited fault sentinel. It must cancel a partial press.
    for _ in 0..4 {
        assert!(!key.sample(0xfffe));
    }
    assert!(!key.sample(0));
    for _ in 0..4 {
        assert!(!key.sample(0xffff));
    }
    let mut unavailable = PageKey::new(0);
    for value in [0xffff, 0xfffe, 0xfffe, 0xfffe, 0xffff, 0xffff, 0xffff] {
        assert!(!unavailable.sample(value));
    }
}
