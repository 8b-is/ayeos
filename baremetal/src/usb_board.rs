//! FDT discovery for the supported MT6768 USB layout. No hardcoded x0 pointer.
use crate::{
    board_screen::Line,
    device_tree::{self, Event},
};
use core::fmt::Write;

#[derive(Clone, Copy)]
struct Node<'a> {
    compatible: &'a [u8],
    reg: &'a [u8],
    ac: u32,
    sc: u32,
    parent_ac: u32,
    parent_sc: u32,
    identity: bool,
    ranges_empty: bool,
    disabled: bool,
    handle: u32,
    phy_cells: u32,
    phys: &'a [u8],
    mode: &'a [u8],
}
const EMPTY: Node<'static> = Node {
    compatible: &[],
    reg: &[],
    ac: 2,
    sc: 1,
    parent_ac: 2,
    parent_sc: 1,
    identity: true,
    ranges_empty: false,
    disabled: false,
    handle: 0,
    phy_cells: u32::MAX,
    phys: &[],
    mode: &[],
};

#[derive(Default, Debug)]
pub struct Board {
    pub soc: bool,
    pub controller: bool,
    pub phy: bool,
    pub infra: bool,
    pub top: bool,
    pub matches: usize,
}
impl Board {
    pub fn ready(&self) -> bool {
        self.soc && self.controller && self.phy && self.infra && self.top && self.matches == 1
    }
}
fn has(bytes: &[u8], text: &[u8]) -> bool {
    bytes.split(|b| *b == 0).any(|b| b == text)
}
fn word(b: &[u8]) -> u32 {
    b.get(..4)
        .map(|s| u32::from_be_bytes(s.try_into().unwrap()))
        .unwrap_or(u32::MAX)
}
fn regions(node: Node<'_>, mut f: impl FnMut(u64, u64)) {
    if !node.identity || node.parent_ac != 2 || node.parent_sc != 2 || node.reg.len() % 16 != 0 {
        return;
    }
    for r in node.reg.chunks_exact(16) {
        f(
            u64::from_be_bytes(r[..8].try_into().unwrap()),
            u64::from_be_bytes(r[8..].try_into().unwrap()),
        );
    }
}
pub fn discover(blob: &[u8]) -> Result<Board, device_tree::Error> {
    let mut board = Board::default();
    let mut stack = [EMPTY; 32];
    let mut depth = 0;
    let mut requested_phy = 0;
    let mut port_handle = 0;
    let mut ports = 0;
    device_tree::walk(blob, |event| match event {
        Event::Begin(_) => {
            let mut n = EMPTY;
            if depth > 0 {
                let p = stack[depth - 1];
                n.parent_ac = p.ac;
                n.parent_sc = p.sc;
                n.identity = depth == 1 || (p.identity && p.ranges_empty);
                n.disabled = p.disabled;
            }
            stack[depth] = n;
            depth += 1;
        }
        Event::Property(name, value) => {
            let n = &mut stack[depth - 1];
            match name {
                "compatible" => n.compatible = value,
                "reg" => n.reg = value,
                "status" => {
                    n.disabled |= value != b"okay\0" && value != b"ok\0";
                }
                "ranges" => n.ranges_empty = value.is_empty(),
                "#address-cells" => n.ac = word(value),
                "#size-cells" => n.sc = word(value),
                "phandle" => n.handle = word(value),
                "#phy-cells" => n.phy_cells = word(value),
                "phys" => n.phys = value,
                "dr_mode" => n.mode = value,
                _ => {}
            }
        }
        Event::End => {
            depth -= 1;
            let n = stack[depth];
            if depth == 0 {
                board.soc =
                    has(n.compatible, b"mediatek,MT6768") || has(n.compatible, b"mediatek,mt6768");
            }
            if n.disabled {
                return;
            }
            let controller = has(n.compatible, b"mediatek,mt6768-usb20");
            if controller {
                board.matches += 1;
                if n.phys.len() == 8 && word(&n.phys[4..]) == 3 {
                    requested_phy = word(n.phys);
                }
            }
            let mut shared = false;
            regions(n, |base, size| {
                if controller
                    && base == 0x1120_0000
                    && size >= 0x400
                    && (n.mode.is_empty() || has(n.mode, b"otg") || has(n.mode, b"peripheral"))
                {
                    board.controller = true;
                }
                if controller && base == 0x11cc_0000 && size >= 0x900 {
                    shared = true;
                }
                if base == 0x11cc_0800
                    && size >= 0x100
                    && n.phy_cells == 1
                    && depth > 0
                    && has(stack[depth - 1].compatible, b"mediatek,generic-tphy-v1")
                {
                    ports += 1;
                    port_handle = n.handle;
                }
                if base == 0x1000_1000
                    && size >= 0x100
                    && has(n.compatible, b"mediatek,infracfg_ao")
                {
                    board.infra = true;
                }
                if base == 0x1000_0000 && size >= 0x100 && has(n.compatible, b"mediatek,topckgen") {
                    board.top = true;
                }
            });
            if controller && !shared {
                board.controller = false;
            }
        }
    })?;
    let mut handle_uses = 0;
    device_tree::walk(blob, |event| {
        if let Event::Property("phandle", value) = event {
            if word(value) == port_handle {
                handle_uses += 1;
            }
        }
    })?;
    board.phy = ports == 1
        && handle_uses == 1
        && port_handle != 0
        && port_handle != u32::MAX
        && port_handle == requested_phy;
    Ok(board)
}

/// Compact one-page fallback, explicit omitted count. Complete bytes go over USB.
pub fn summary(blob: &[u8], mut emit: impl FnMut(&str)) {
    let mut names = [""; 32];
    let mut selected = [false; 32];
    let mut depth = 0;
    let mut shown = 0;
    let mut omitted = 0;
    let _ = device_tree::walk(blob, |e| match e {
        Event::Begin(name) => {
            names[depth] = name;
            selected[depth] =
                name.contains("usb") || name.contains("musb") || name.contains("u2phy");
            depth += 1;
        }
        Event::End => depth -= 1,
        Event::Property(name, value) => {
            if name == "compatible" && value.windows(3).any(|w| w == b"usb") {
                selected[depth - 1] = true;
            }
            if !selected[depth - 1]
                || !(matches!(
                    name,
                    "compatible"
                        | "reg"
                        | "reg-names"
                        | "status"
                        | "clocks"
                        | "clock-names"
                        | "phys"
                        | "phy-names"
                        | "dr_mode"
                        | "#phy-cells"
                        | "phandle"
                        | "#address-cells"
                        | "#size-cells"
                        | "regulator-name"
                        | "regulator-min-microvolt"
                        | "regulator-max-microvolt"
                ) || name.ends_with("-supply"))
            {
                return;
            }
            if shown >= 24 {
                omitted += 1;
                return;
            }
            shown += 1;
            let mut path = Line::new();
            for part in &names[..depth] {
                let _ = write!(path, "{}/", part);
            }
            emit(path.text());
            let mut line = Line::new();
            let _ = write!(line, " {} [{}B] ", name, value.len());
            if matches!(
                name,
                "compatible"
                    | "status"
                    | "clock-names"
                    | "phy-names"
                    | "dr_mode"
                    | "regulator-name"
                    | "reg-names"
            ) {
                for b in value.iter().take(76) {
                    let _ = write!(
                        line,
                        "{}",
                        if *b == 0 {
                            '|'
                        } else if b.is_ascii_graphic() {
                            *b as char
                        } else {
                            '.'
                        }
                    );
                }
            } else {
                for b in value.iter().take(24) {
                    let _ = write!(line, "{:02x}", b);
                }
                if value.len() > 24 {
                    let _ = write!(line, " ~");
                }
            }
            emit(line.text());
        }
    });
    let mut line = Line::new();
    let _ = write!(
        line,
        "USB FDT records shown={} omitted={} (~ = truncated)",
        shown, omitted
    );
    emit(line.text());
}
