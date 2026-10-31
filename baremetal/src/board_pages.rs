//! Board3: read-only, bounded resource inventory and lossless paged hex records.
//! No MMIO, allocation, radio traffic, firmware loading or updater calls.
use crate::board_screen::{category, Line};
use crate::device_tree::{self, Event};
use core::fmt::{self, Write};

pub const PAGE_ROWS: usize = 68;
pub const MAX_LINES: usize = 4096;
const MAX_SELECTED: usize = 128;
const MAX_PROVIDERS: usize = 512;
const NONE: u32 = u32::MAX;
const BUS: u8 = 3;
const DEPENDENCY: u8 = 4;

// #clock-cells, #reset-cells, #power-domain-cells, #gpio-cells, #sound-dai-cells.
const CELL_NAMES: [&str; 5] = [
    "#clock-cells",
    "#reset-cells",
    "#power-domain-cells",
    "#gpio-cells",
    "#sound-dai-cells",
];

#[derive(Clone, Copy)]
struct Node {
    id: u32,
    parent: u32,
    group: u8,
    subtree: bool,
}
const EMPTY_NODE: Node = Node {
    id: NONE,
    parent: NONE,
    group: BUS,
    subtree: false,
};

#[derive(Clone, Copy)]
struct Provider {
    node: Node,
    handle: u32,
    cells: [u32; 5],
}
const EMPTY_PROVIDER: Provider = Provider {
    node: EMPTY_NODE,
    handle: 0,
    cells: [NONE; 5],
};

pub struct Inventory {
    selected: [Node; MAX_SELECTED],
    used: usize,
    providers: [Provider; MAX_PROVIDERS],
    providers_used: usize,
    pub node_overflow: usize,
    pub provider_overflow: usize,
    pub unresolved: usize,
    pub malformed_refs: usize,
    pub duplicate_handles: usize,
    pub crc: u32,
}

fn cell(bytes: &[u8]) -> Option<u32> {
    Some(u32::from_be_bytes(bytes.get(..4)?.try_into().ok()?))
}

/// IEEE CRC-32: corruption/transcription detection only, not authentication.
pub fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & 0u32.wrapping_sub(crc & 1));
        }
    }
    !crc
}

impl Inventory {
    fn add(&mut self, node: Node) {
        if let Some(old) = self.selected[..self.used]
            .iter_mut()
            .find(|n| n.id == node.id)
        {
            // A real target retains priority over a bus/dependency insertion.
            if node.group < old.group {
                old.group = node.group;
            }
            old.subtree |= node.subtree;
        } else if self.used < MAX_SELECTED {
            self.selected[self.used] = node;
            self.used += 1;
        } else {
            self.node_overflow += 1;
        }
    }

    pub fn build(blob: &[u8]) -> Result<Self, device_tree::Error> {
        device_tree::walk(blob, |_| {})?;
        let mut out = Self {
            selected: [EMPTY_NODE; MAX_SELECTED],
            used: 0,
            providers: [EMPTY_PROVIDER; MAX_PROVIDERS],
            providers_used: 0,
            node_overflow: 0,
            provider_overflow: 0,
            unresolved: 0,
            malformed_refs: 0,
            duplicate_handles: 0,
            crc: crc32(blob),
        };
        let mut stack = [EMPTY_PROVIDER; 32];
        let mut blocked = [false; 32];
        let mut depth = 0;
        let mut next = 0;
        device_tree::walk(blob, |event| match event {
            Event::Begin(name) => {
                let parent = if depth == 0 {
                    NONE
                } else {
                    stack[depth - 1].node.id
                };
                let excluded = name == "reserved-memory"
                    || name == "pinctrl"
                    || (depth > 0 && blocked[depth - 1]);
                stack[depth] = EMPTY_PROVIDER;
                stack[depth].node = Node {
                    id: next,
                    parent,
                    group: category(name).map_or(BUS, |g| g as u8),
                    subtree: false,
                };
                blocked[depth] = excluded;
                next += 1;
                depth += 1;
            }
            Event::Property(name, value) => {
                let node = &mut stack[depth - 1];
                if name == "compatible" && node.node.group == BUS {
                    node.node.group = core::str::from_utf8(value)
                        .ok()
                        .and_then(category)
                        .map_or(BUS, |g| g as u8);
                }
                if matches!(name, "phandle" | "linux,phandle") {
                    if value.len() == 4 {
                        let handle = cell(value).unwrap_or(0);
                        if node.handle != 0 && node.handle != handle {
                            out.malformed_refs += 1;
                        }
                        node.handle = handle;
                    } else {
                        out.malformed_refs += 1;
                    }
                }
                if let Some(i) = CELL_NAMES.iter().position(|n| *n == name) {
                    if value.len() == 4 {
                        node.cells[i] = cell(value).unwrap_or(NONE);
                    } else {
                        out.malformed_refs += 1;
                    }
                }
            }
            Event::End => {
                depth -= 1;
                let node = stack[depth];
                if !blocked[depth] && node.node.group < BUS {
                    out.add(node.node);
                    // Retain ancestors' bus cell counts, ranges and resources.
                    for ancestor in &stack[..depth] {
                        out.add(Node {
                            group: BUS,
                            ..ancestor.node
                        });
                    }
                }
                if node.handle != 0 && node.handle != NONE {
                    if let Some(old) = out.providers[..out.providers_used]
                        .iter_mut()
                        .find(|p| p.handle == node.handle)
                    {
                        old.node.id = NONE; // Ambiguous phandle cannot resolve safely.
                        out.duplicate_handles += 1;
                    } else if out.providers_used < MAX_PROVIDERS {
                        out.providers[out.providers_used] = node;
                        out.providers_used += 1;
                    } else {
                        out.provider_overflow += 1;
                    }
                }
            }
        })?;
        // Resolve DIRECT dependencies of target nodes and their ancestor buses.
        // This is not a recursive transitive closure over all clock providers.
        let original = out.used;
        visit(blob, |id, _, _, _, name, value| {
            if !out.selected[..original].iter().any(|n| n.id == id) {
                return;
            }
            let kind = reference_kind(name);
            if let Some(kind) = kind {
                out.references(value, kind);
            }
        });
        // Keep address/range context for directly resolved provider nodes too.
        // The ancestry pass uses stable IDs and never widens into siblings.
        let mut ids = [NONE; 32];
        let mut depth = 0;
        let mut next = 0;
        device_tree::walk(blob, |event| match event {
            Event::Begin(_) => {
                ids[depth] = next;
                next += 1;
                depth += 1;
                if out.selected[..out.used]
                    .iter()
                    .any(|n| n.id == ids[depth - 1])
                {
                    for i in 0..depth - 1 {
                        out.add(Node {
                            id: ids[i],
                            parent: if i == 0 { NONE } else { ids[i - 1] },
                            group: BUS,
                            subtree: false,
                        });
                    }
                }
            }
            Event::End => depth -= 1,
            _ => {}
        })?;
        Ok(out)
    }

    fn references(&mut self, value: &[u8], kind: RefKind) {
        if value.len() % 4 != 0 || (matches!(kind, RefKind::Single) && value.len() != 4) {
            self.malformed_refs += 1;
            return;
        }
        let mut offset = 0;
        while offset < value.len() {
            let handle = cell(&value[offset..]).unwrap_or(0);
            // Zero placeholders are valid in some phandle arrays (e.g. GPIO).
            if handle == 0 {
                offset += 4;
                continue;
            }
            let Some(provider) = self.providers[..self.providers_used]
                .iter()
                .find(|p| p.handle == handle && p.node.id != NONE)
                .copied()
            else {
                self.unresolved += 1;
                // Unknown specifier size: stop, do not treat arguments as phandles.
                if matches!(kind, RefKind::Cells(_)) {
                    return;
                }
                offset += 4;
                continue;
            };
            self.add(Node {
                group: DEPENDENCY,
                subtree: matches!(kind, RefKind::List),
                ..provider.node
            });
            let args = match kind {
                RefKind::Cells(i) => provider.cells[i],
                _ => 0,
            };
            if args == NONE || u64::from(args) + 1 > ((value.len() - offset) / 4) as u64 {
                self.malformed_refs += 1;
                return;
            }
            offset += (args as usize + 1) * 4;
        }
    }

    pub fn page_count(&self, blob: &[u8]) -> (usize, bool) {
        let stats = self.render(blob, usize::MAX, |_| {});
        (
            (stats.lines.max(1) + PAGE_ROWS - 1) / PAGE_ROWS,
            stats.capped,
        )
    }

    pub fn render(&self, blob: &[u8], page: usize, mut emit: impl FnMut(&str)) -> RenderStats {
        let mut stream = Stream {
            page,
            lines: 0,
            capped: false,
            emit: &mut emit,
        };
        let mut record = 0;
        // Connectivity first: this is the next command/event transport milestone.
        for (group, title) in [
            (2, "CONNSYS / WLAN / FM"),
            (0, "AUDIO / AFE"),
            (1, "AMPLIFIERS"),
            (BUS, "PARENT BUSES"),
            (DEPENDENCY, "DIRECT DEPENDENCIES"),
        ] {
            stream.line(format_args!("SECTION {}", title));
            let mut last = NONE;
            visit(blob, |id, parent, ids, path, name, value| {
                let selected =
                    if let Some(node) = self.selected[..self.used].iter().find(|n| n.id == id) {
                        node.group == group
                    } else {
                        group == DEPENDENCY
                            && self.selected[..self.used]
                                .iter()
                                .any(|n| n.subtree && ids.contains(&n.id))
                    };
                if stream.capped || !allowed(name) || !selected {
                    return;
                }
                if last != id {
                    last = id;
                    stream.line(format_args!("NODE {:04x} PARENT {:08x}", id, parent));
                    // Exact path chunks: no truncation, even for deep/long paths.
                    let mut bytes = [0u8; 8192];
                    let mut len = 0;
                    for component in path {
                        bytes[len..len + component.len()].copy_from_slice(component.as_bytes());
                        len += component.len();
                        bytes[len] = b'/';
                        len += 1;
                    }
                    for (i, chunk) in bytes[..len].chunks(60).enumerate() {
                        stream.line(format_args!(
                            "PATH+{:04x} {}",
                            i * 60,
                            core::str::from_utf8(chunk).unwrap_or("invalid")
                        ));
                    }
                }
                for (i, chunk) in name.as_bytes().chunks(52).enumerate() {
                    stream.line(format_args!(
                        "R{:04x} NAME+{:04x} {}",
                        record,
                        i * 52,
                        core::str::from_utf8(chunk).unwrap_or("invalid")
                    ));
                }
                stream.line(format_args!(
                    "R{:04x} BYTES={} CRC32={:08x}",
                    record,
                    value.len(),
                    crc32(value)
                ));
                for (i, chunk) in value.chunks(24).enumerate() {
                    if stream.capped {
                        break;
                    }
                    let mut line = Line::new();
                    let _ = write!(line, "R{:04x} +{:06x} ", record, i * 24);
                    for byte in chunk {
                        let _ = write!(line, "{:02x}", byte);
                    }
                    stream.text(line.text());
                }
                record += 1;
            });
        }
        stream.line(format_args!(
            "END records={} (selected allowlist; no driver readiness claim)",
            record
        ));
        RenderStats {
            lines: stream.lines,
            capped: stream.capped,
        }
    }
}

#[derive(Clone, Copy)]
enum RefKind {
    Cells(usize),
    Single,
    List,
}

fn reference_kind(name: &str) -> Option<RefKind> {
    match name {
        "clocks" => Some(RefKind::Cells(0)),
        "resets" => Some(RefKind::Cells(1)),
        "power-domains" => Some(RefKind::Cells(2)),
        "sound-dai" => Some(RefKind::Cells(4)),
        "interrupt-parent" => Some(RefKind::Single),
        "gpio" | "gpios" => Some(RefKind::Cells(3)),
        _ if name.ends_with("-gpio") || name.ends_with("-gpios") => Some(RefKind::Cells(3)),
        _ if name.ends_with("-supply") => Some(RefKind::Single),
        _ if name
            .strip_prefix("pinctrl-")
            .is_some_and(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())) =>
        {
            Some(RefKind::List)
        }
        _ => None,
    }
}

fn allowed(name: &str) -> bool {
    reference_kind(name).is_some()
        || CELL_NAMES.contains(&name)
        || matches!(
            name,
            "compatible"
                | "status"
                | "reg"
                | "reg-names"
                | "phandle"
                | "linux,phandle"
                | "#address-cells"
                | "#size-cells"
                | "ranges"
                | "dma-ranges"
                | "interrupts"
                | "interrupts-extended"
                | "interrupt-names"
                | "#interrupt-cells"
                | "interrupt-controller"
                | "clock-names"
                | "clock-frequency"
                | "reset-names"
                | "pinctrl-names"
                | "gpio-controller"
                | "gpio-ranges"
                | "pinmux"
                | "pins"
                | "function"
                | "drive-strength"
                | "bias-pull-up"
                | "bias-pull-down"
                | "bias-disable"
                | "input-enable"
                | "output-high"
                | "output-low"
                | "regulator-name"
                | "regulator-min-microvolt"
                | "regulator-max-microvolt"
                | "regulator-boot-on"
                | "regulator-always-on"
        )
}

fn visit<'a>(blob: &'a [u8], mut f: impl FnMut(u32, u32, &[u32], &[&'a str], &'a str, &'a [u8])) {
    let mut names = [""; 32];
    let mut ids = [NONE; 32];
    let mut depth = 0;
    let mut next = 0;
    let _ = device_tree::walk(blob, |event| match event {
        Event::Begin(name) => {
            names[depth] = name;
            ids[depth] = next;
            next += 1;
            depth += 1;
        }
        Event::End => depth -= 1,
        Event::Property(name, value) => f(
            ids[depth - 1],
            if depth < 2 { NONE } else { ids[depth - 2] },
            &ids[..depth],
            &names[..depth],
            name,
            value,
        ),
    });
}

pub struct RenderStats {
    pub lines: usize,
    pub capped: bool,
}
struct Stream<'a, F> {
    page: usize,
    lines: usize,
    capped: bool,
    emit: &'a mut F,
}
impl<F: FnMut(&str)> Stream<'_, F> {
    fn text(&mut self, text: &str) {
        if self.lines == MAX_LINES {
            self.capped = true;
            return;
        }
        if self.lines / PAGE_ROWS == self.page {
            (self.emit)(text);
        }
        self.lines += 1;
    }
    fn line(&mut self, args: fmt::Arguments<'_>) {
        let mut line = Line::new();
        let _ = line.write_fmt(args);
        self.text(line.text());
    }
}

/// Sample-count debounce, not a measured time interval. A stable changed
/// sample followed by a stable release emits one event; holding never repeats.
pub struct PageKey {
    idle: u16,
    candidate: u16,
    count: u8,
    pressed: bool,
}
impl PageKey {
    pub fn new(idle: u16) -> Self {
        Self {
            idle,
            candidate: idle,
            count: 0,
            pressed: false,
        }
    }
    pub fn sample(&mut self, value: u16) -> bool {
        if self.idle == 0 || value == 0 {
            self.pressed = false;
            self.count = 0;
            return false;
        }
        if value != self.candidate {
            self.candidate = value;
            self.count = 1;
            return false;
        }
        self.count = self.count.saturating_add(1);
        if self.count < 3 {
            return false;
        }
        if value != self.idle {
            self.pressed = true;
            false
        } else {
            let event = self.pressed;
            self.pressed = false;
            event
        }
    }
}
