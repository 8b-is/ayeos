#[path = "../src/board_screen.rs"]
mod board_screen;
#[path = "../src/device_tree.rs"]
mod device_tree;

#[test]
fn selected_resources_fit_stationary_page_and_report_omissions() {
    let mut structure = Vec::new();
    for w in [1u32, 0, 1] {
        structure.extend(w.to_be_bytes());
    }
    structure.extend(b"sound\0\0\0");
    for _ in 0..40 {
        for w in [3u32, 4, 0, 0x12345678] {
            structure.extend(w.to_be_bytes());
        }
    }
    for w in [2u32, 2, 9] {
        structure.extend(w.to_be_bytes());
    }
    let strings_at = 56 + structure.len();
    let mut blob = vec![0; 56];
    blob.extend(&structure);
    blob.extend(b"reg\0");
    let total = blob.len() as u32;
    for (i, v) in [
        0xd00dfeed,
        total,
        56,
        strings_at as u32,
        40,
        17,
        16,
        0,
        4,
        structure.len() as u32,
    ]
    .iter()
    .enumerate()
    {
        blob[i * 4..i * 4 + 4].copy_from_slice(&v.to_be_bytes());
    }
    let mut lines = Vec::new();
    board_screen::resources(&blob, |s| lines.push(s.to_owned()));
    assert!(lines.len() + 8 <= 85);
    assert!(lines.iter().all(|s| s.len() <= 76 && s.is_ascii()));
    assert!(lines.iter().any(|s| s.contains("shown=8 omitted=32")));
    assert!(lines.iter().any(|s| s.contains("12345678")));
}

#[test]
fn pmic_children_cannot_starve_groups_and_clock_lookup_is_order_independent() {
    fn word(s: &mut Vec<u8>, w: u32) {
        s.extend(w.to_be_bytes());
    }
    fn begin(s: &mut Vec<u8>, name: &str) {
        word(s, 1);
        s.extend(name.as_bytes());
        s.push(0);
        while s.len() % 4 != 0 {
            s.push(0);
        }
    }
    fn prop(s: &mut Vec<u8>, strings: &mut Vec<u8>, name: &str, value: &[u8]) {
        let offset = strings.len();
        strings.extend(name.as_bytes());
        strings.push(0);
        for w in [3, value.len() as u32, offset as u32] {
            word(s, w);
        }
        s.extend(value);
        while s.len() % 4 != 0 {
            s.push(0);
        }
    }
    let mut s = Vec::new();
    let mut strings = Vec::new();
    begin(&mut s, "");
    begin(&mut s, "pwrap@1000d000");
    begin(&mut s, "mt6358-pmic");
    for _ in 0..40 {
        begin(&mut s, "mt6358rtc");
        prop(&mut s, &mut strings, "compatible", b"mediatek,mt6358-rtc\0");
        word(&mut s, 2);
    }
    word(&mut s, 2);
    word(&mut s, 2);
    begin(&mut s, "clock@10000000");
    prop(&mut s, &mut strings, "#clock-cells", &1u32.to_be_bytes());
    prop(&mut s, &mut strings, "reg", &[0; 16]);
    prop(&mut s, &mut strings, "phandle", &0x12u32.to_be_bytes());
    word(&mut s, 2);
    for (name, compatible) in [
        ("audio@11220000", "mediatek,mt6768-afe"),
        ("chip@34", "awinic,aw87339"),
        ("consys@18000000", "mediatek,consys"),
    ] {
        begin(&mut s, name);
        // Deliberately put reg before compatible: preselection must still show it.
        prop(&mut s, &mut strings, "reg", &0x12345678u32.to_be_bytes());
        prop(&mut s, &mut strings, "compatible", compatible.as_bytes());
        prop(
            &mut s,
            &mut strings,
            "clocks",
            &[0, 0, 0, 0x12, 0, 0, 0, 0x34],
        );
        word(&mut s, 2);
    }
    word(&mut s, 2);
    word(&mut s, 9);
    let mut blob = vec![0; 56];
    blob.extend(&s);
    blob.extend(&strings);
    let header = [
        0xd00dfeed,
        blob.len() as u32,
        56,
        (56 + s.len()) as u32,
        40,
        17,
        16,
        0,
        strings.len() as u32,
        s.len() as u32,
    ];
    for (i, w) in header.iter().enumerate() {
        blob[4 * i..4 * i + 4].copy_from_slice(&w.to_be_bytes());
    }
    let mut lines = Vec::new();
    board_screen::resources(&blob, |l| lines.push(l.to_owned()));
    let text = lines.join("\n");
    assert!(!text.contains("mt6358rtc"));
    for name in [
        "audio@11220000",
        "chip@34",
        "consys@18000000",
        "clock@10000000",
        "#clock-cells",
        "unresolved=0",
    ] {
        assert!(text.contains(name), "missing {name}: {text}");
    }
    assert_eq!(text.matches("12345678").count(), 3);
    assert!(lines.len() + 8 <= 85);
}
