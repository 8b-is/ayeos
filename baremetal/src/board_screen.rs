//! Fixed-capacity, ASCII-only diagnostic lines. No allocation or hardware I/O.
use core::fmt::{self, Write};
use crate::device_tree::{self, Event};

pub struct Line {
    bytes: [u8; 76],
    len: usize,
}
impl Line {
    pub fn new() -> Self { Self { bytes: [0; 76], len: 0 } }
    pub fn text(&self) -> &str { core::str::from_utf8(&self.bytes[..self.len]).unwrap_or("invalid text") }
}
impl Write for Line {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        for byte in text.bytes() {
            if self.len == self.bytes.len() {
                self.bytes[75] = b'~';
                break;
            }
            self.bytes[self.len] = if (32..=126).contains(&byte) { byte } else { b'.' };
            self.len += 1;
        }
        Ok(())
    }
}

/// Validate before emitting any resource, then show bounded selected records.
pub fn resources(blob: &[u8], mut emit: impl FnMut(&str)) {
    if let Err(error) = device_tree::walk(blob, |_| {}) {
        let mut line = Line::new();
        let _ = write!(line, "FDT PARSE FAILED: {:?}", error);
        emit(line.text());
        return;
    }
    emit("FDT PARSE OK - declarations, not hardware readiness");
    emit("Values: big-endian raw bytes; ~ means line truncated");
    let mut names = [""; 32];
    let mut depth = 0usize;
    let mut shown = 0usize;
    let mut omitted = 0usize;
    let result = device_tree::walk(blob, |event| match event {
        Event::Begin(name) => { names[depth] = name; depth += 1; }
        Event::End => depth -= 1,
        Event::Property(name, value) => {
            let selected = names[..depth].iter().any(|part|
                ["pwrap", "6358", "sound", "audio", "afe", "aw87", "consys", "conn", "wifi", "wlan", "fm"].iter().any(|key| part.contains(key)));
            if !(selected || depth == 1) || !matches!(name,
                "compatible" | "reg" | "status" | "clocks" | "phandle" | "#address-cells" | "#size-cells") { return; }
            if shown >= 32 { omitted += 1; return; }
            shown += 1;
            let mut line = Line::new();
            for part in &names[..depth] { let _ = write!(line, "{}/", part); }
            emit(line.text());
            let mut line = Line::new();
            let _ = write!(line, " {} [{}B] ", name, value.len());
            if matches!(name, "compatible" | "status") {
                for &b in value.iter().take(76) {
                    let c = if b == 0 { '|' } else if (32..=126).contains(&b) { b as char } else { '.' };
                    let _ = write!(line, "{}", c);
                }
                if value.len() > 76 { let _ = write!(line, "~"); }
            } else {
                for b in value.iter().take(16) { let _ = write!(line, "{:02x}", b); }
                if value.len() > 16 { let _ = write!(line, " ~"); }
            }
            emit(line.text());
        }
    });
    let mut line = Line::new();
    let _ = write!(line, "Records shown={} omitted={} parse={:?}", shown, omitted, result);
    emit(line.text());
    emit("END - photograph this stationary page");
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn text_stays_ascii_and_marks_overflow() {
        let mut line = Line::new();
        let _ = write!(line, "{}", "é".repeat(100));
        assert_eq!(line.text().len(), 76);
        assert!(line.text().is_ascii());
        assert!(line.text().ends_with('~'));
    }
    #[test]
    fn malformed_blob_never_emits_resources() {
        let mut lines = Vec::new();
        resources(&[0; 40], |s| lines.push(s.to_owned()));
        assert_eq!(lines.len(), 1);
        assert!(lines[0].starts_with("FDT PARSE FAILED"));
    }
}
