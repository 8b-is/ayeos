//! Fixed-capacity, ASCII-only diagnostic lines. No allocation or hardware I/O.
use crate::device_tree::{self, Event};
use core::fmt::{self, Write};

pub struct Line {
    bytes: [u8; 76],
    len: usize,
}
impl Line {
    pub fn new() -> Self {
        Self {
            bytes: [0; 76],
            len: 0,
        }
    }
    pub fn text(&self) -> &str {
        core::str::from_utf8(&self.bytes[..self.len]).unwrap_or("invalid text")
    }
}
impl Write for Line {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        for byte in text.bytes() {
            if self.len == self.bytes.len() {
                self.bytes[75] = b'~';
                break;
            }
            self.bytes[self.len] = if (32..=126).contains(&byte) {
                byte
            } else {
                b'.'
            };
            self.len += 1;
        }
        Ok(())
    }
}

fn cell(value: &[u8]) -> Option<u32> {
    Some(u32::from_be_bytes(value.get(..4)?.try_into().ok()?))
}

// Classify the node itself, never all descendants of a PMIC or bus.
pub(crate) fn category(text: &str) -> Option<usize> {
    if ["aw87", "amplifier", "speaker-amp"]
        .iter()
        .any(|s| text.contains(s))
    {
        Some(1)
    } else if ["sound", "audio", "afe"].iter().any(|s| text.contains(s)) {
        Some(0)
    } else if ["consys", "connectivity", "wlan", "wifi", "fmsys"]
        .iter()
        .any(|s| text.contains(s))
    {
        Some(2)
    } else {
        None
    }
}

/// Visit properties with stable preorder node IDs, path, and parent context.
fn properties<'a>(blob: &'a [u8], mut f: impl FnMut(usize, &[&'a str], &'a str, &'a [u8])) {
    let mut names = [""; 32];
    let mut ids = [0; 32];
    let mut next = 0;
    let mut depth = 0;
    let _ = device_tree::walk(blob, |event| match event {
        Event::Begin(name) => {
            names[depth] = name;
            ids[depth] = next;
            next += 1;
            depth += 1;
        }
        Event::End => depth -= 1,
        Event::Property(name, value) => f(ids[depth - 1], &names[..depth], name, value),
    });
}

/// Four independent budgets prevent PMIC descendants starving later resources.
pub fn resources(blob: &[u8], mut emit: impl FnMut(&str)) {
    if let Err(error) = device_tree::walk(blob, |_| {}) {
        let mut line = Line::new();
        let _ = write!(line, "FDT PARSE FAILED: {:?}", error);
        emit(line.text());
        return;
    }
    emit("FDT PARSE OK - declarations, not hardware readiness");
    emit("Raw BE bytes; ~ truncated. Clock lookup: first provider per clocks.");
    let mut selected = [(usize::MAX, 0usize); 128];
    let mut used = 0;
    let mut overflow = 0;
    let mut last_overflow = usize::MAX;
    properties(blob, |id, names, name, value| {
        if names
            .iter()
            .any(|n| *n == "reserved-memory" || *n == "pinctrl")
        {
            return;
        }
        let kind = category(names[names.len() - 1]).or_else(|| {
            if name == "compatible" {
                core::str::from_utf8(value).ok().and_then(category)
            } else {
                None
            }
        });
        if let Some(kind) = kind {
            if selected[..used].iter().any(|(n, _)| *n == id) {
                return;
            }
            if used < selected.len() {
                selected[used] = (id, kind);
                used += 1;
            } else if last_overflow != id {
                overflow += 1;
                last_overflow = id;
            }
        }
    });
    // Capture only the first specifier, whose first cell is unambiguously a
    // provider phandle. Later cells need each provider's #clock-cells.
    let mut refs = [0u32; 16];
    refs[0] = 0x12; // observed PWRAP first provider in Board1 capture
    let mut refs_used = 1;
    let mut refs_omitted = 0;
    properties(blob, |id, _, name, value| {
        if name == "clocks" && selected[..used].iter().any(|(n, _)| *n == id) {
            if let Some(handle) = cell(value) {
                if !refs[..refs_used].contains(&handle) {
                    if refs_used < refs.len() {
                        refs[refs_used] = handle;
                        refs_used += 1;
                    } else {
                        refs_omitted += 1;
                    }
                }
            }
        }
    });
    let mut providers = [usize::MAX; 16];
    properties(blob, |id, _, name, value| {
        if matches!(name, "phandle" | "linux,phandle") {
            if let Some(handle) = cell(value) {
                if let Some(i) = refs[..refs_used].iter().position(|h| *h == handle) {
                    providers[i] = id;
                }
            }
        }
    });
    for (group, title) in [
        "AUDIO / AFE",
        "AMPLIFIER",
        "CONNSYS / WLAN / FM",
        "CLOCK PROVIDERS",
    ]
    .iter()
    .enumerate()
    {
        emit(title);
        let mut shown = 0;
        let mut omitted = 0;
        properties(blob, |id, names, name, value| {
            let matches = if group == 3 {
                providers[..refs_used].contains(&id)
            } else {
                selected[..used].contains(&(id, group))
            };
            if !matches
                || !matches!(
                    name,
                    "compatible"
                        | "reg"
                        | "status"
                        | "clocks"
                        | "clock-names"
                        | "#clock-cells"
                        | "phandle"
                        | "interrupts"
                        | "pinctrl-0"
                        | "power-domains"
                        | "sound-dai"
                        | "#address-cells"
                        | "#size-cells"
                        | "ranges"
                )
            {
                return;
            }
            if shown == 8 {
                omitted += 1;
                return;
            }
            shown += 1;
            let mut line = Line::new();
            for part in names {
                let _ = write!(line, "{}/", part);
            }
            emit(line.text());
            let mut line = Line::new();
            let _ = write!(line, " {} [{}B] ", name, value.len());
            if matches!(name, "compatible" | "status" | "clock-names") {
                for &b in value.iter().take(76) {
                    let c = if b == 0 {
                        '|'
                    } else if (32..=126).contains(&b) {
                        b as char
                    } else {
                        '.'
                    };
                    let _ = write!(line, "{}", c);
                }
                if value.len() > 76 {
                    let _ = write!(line, "~");
                }
            } else {
                for b in value.iter().take(16) {
                    let _ = write!(line, "{:02x}", b);
                }
                if value.len() > 16 {
                    let _ = write!(line, " ~");
                }
            }
            emit(line.text());
        });
        let mut line = Line::new();
        let _ = write!(line, "shown={} omitted={}", shown, omitted);
        emit(line.text());
    }
    let mut line = Line::new();
    let missing = providers[..refs_used]
        .iter()
        .filter(|id| **id == usize::MAX)
        .count();
    let _ = write!(
        line,
        "Node overflow={} clock refs omitted={} unresolved={}",
        overflow, refs_omitted, missing
    );
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
