#[path = "../src/device_tree.rs"]
mod device_tree;
#[path = "../src/board_screen.rs"]
mod board_screen;

#[test]
fn selected_resources_fit_stationary_page_and_report_omissions() {
    let mut structure = Vec::new();
    for w in [1u32, 0, 1] { structure.extend(w.to_be_bytes()); }
    structure.extend(b"sound\0\0\0");
    for _ in 0..40 {
        for w in [3u32, 4, 0, 0x12345678] { structure.extend(w.to_be_bytes()); }
    }
    for w in [2u32, 2, 9] { structure.extend(w.to_be_bytes()); }
    let strings_at = 56 + structure.len();
    let mut blob = vec![0; 56];
    blob.extend(&structure);
    blob.extend(b"reg\0");
    let total = blob.len() as u32;
    for (i, v) in [0xd00dfeed, total, 56, strings_at as u32, 40, 17, 16, 0, 4, structure.len() as u32].iter().enumerate() {
        blob[i * 4..i * 4 + 4].copy_from_slice(&v.to_be_bytes());
    }
    let mut lines = Vec::new();
    board_screen::resources(&blob, |s| lines.push(s.to_owned()));
    assert!(lines.len() + 8 <= 85);
    assert!(lines.iter().all(|s| s.len() <= 76 && s.is_ascii()));
    assert!(lines.iter().any(|s| s.contains("shown=32 omitted=8")));
    assert!(lines.iter().any(|s| s.contains("12345678")));
}
