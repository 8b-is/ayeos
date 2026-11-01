//! Board5's narrowly supported I2C4 profile. FDT declarations, not bus evidence.
use crate::device_tree::{self, Event};

#[derive(Clone, Copy, Default)]
struct Node<'a> {
    name: &'a str,
    compatible: &'a [u8],
    reg: &'a [u8],
    clocks: &'a [u8],
    names: &'a [u8],
    handle: u32,
    ac: Option<u32>,
    sc: Option<u32>,
    clock_cells: Option<u32>,
    frequency: Option<u32>,
    divider: Option<u32>,
    index: Option<u32>,
    disabled: bool,
    unsupported: bool,
    open_drain: bool,
    irq_sel: bool,
    rs_stop: bool,
}
fn word(b: &[u8]) -> Option<u32> {
    Some(u32::from_be_bytes(b.try_into().ok()?))
}
fn has(b: &[u8], s: &[u8]) -> bool {
    b.last() == Some(&0) && b.split(|c| *c == 0).any(|v| v == s)
}
fn regs(b: &[u8], expected: &[(u64, u64)]) -> bool {
    b.len() == expected.len() * 16
        && b.chunks_exact(16).zip(expected).all(|(r, &(a, n))| {
            u64::from_be_bytes(r[..8].try_into().unwrap()) == a
                && u64::from_be_bytes(r[8..].try_into().unwrap()) == n
        })
}

/// Only direct-root identity mapping is supported. Resolve the clock provider,
/// never assume that a numeric phandle is stable across bootloader revisions.
pub fn matches(blob: &[u8]) -> Result<bool, device_tree::Error> {
    let mut stack = [Node::default(); 32];
    let mut depth = 0;
    let mut root = false;
    let mut good = [0u32; 6]; // infra, top, gpio, i2c4, PA0, PA1
    let mut seen = [0u32; 6];
    let mut infra_handle = 0;
    let mut i2c_clocks: &[u8] = &[];
    device_tree::walk(blob, |event| match event {
        Event::Begin(name) => {
            stack[depth] = Node {
                name,
                disabled: depth > 0 && stack[depth - 1].disabled,
                ..Node::default()
            };
            depth += 1;
        }
        Event::Property(name, v) => {
            let n = &mut stack[depth - 1];
            match name {
                "compatible" => n.compatible = v,
                "reg" => n.reg = v,
                "clocks" => n.clocks = v,
                "clock-names" => n.names = v,
                "phandle" => n.handle = word(v).unwrap_or(0),
                "#address-cells" => n.ac = word(v),
                "#size-cells" => n.sc = word(v),
                "#clock-cells" => n.clock_cells = word(v),
                "clock-frequency" => n.frequency = word(v),
                "clock-div" => n.divider = word(v),
                "dev_index" => n.index = word(v),
                "status" => n.disabled |= v != b"okay\0" && v != b"ok\0",
                "mediatek,use-open-drain" => n.open_drain = v.is_empty(),
                "mediatek,control-irq-sel" => n.irq_sel = v.is_empty(),
                "mediatek,control-rs-stop" => n.rs_stop = v.is_empty(),
                // GPIO controller's boot defaults are not a consumer reset GPIO.
                // The probe checks the actual mode register, never applies this table.
                "gpio_init_default" if n.name == "gpio@10005000" => {}
                _ if name.contains("gpio")
                    || name.ends_with("-supply")
                    || name.starts_with("pinctrl-")
                    || name == "power-domains"
                    || name == "ranges" =>
                {
                    n.unsupported = true
                }
                _ => {}
            }
        }
        Event::End => {
            depth -= 1;
            let n = stack[depth];
            if depth == 0 {
                root = !n.disabled
                    && n.ac == Some(2)
                    && n.sc == Some(2)
                    && (has(n.compatible, b"mediatek,MT6768")
                        || has(n.compatible, b"mediatek,mt6768"));
            }
            let candidate = match (depth, n.name) {
                (1, "infracfg_ao@10001000") => Some((
                    0,
                    has(n.compatible, b"mediatek,infracfg_ao")
                        && regs(n.reg, &[(0x10001000, 0x1000)])
                        && n.clock_cells == Some(1),
                )),
                (1, "topckgen@10000000") => Some((
                    1,
                    has(n.compatible, b"mediatek,topckgen") && regs(n.reg, &[(0x10000000, 0x1000)]),
                )),
                (1, "gpio@10005000") => Some((
                    2,
                    has(n.compatible, b"mediatek,gpio") && regs(n.reg, &[(0x10005000, 0x1000)]),
                )),
                (1, "i2c@11004000") => Some((
                    3,
                    has(n.compatible, b"mediatek,mt6768-i2c")
                        && regs(n.reg, &[(0x11004000, 0x1000), (0x11000680, 0x180)])
                        && n.ac == Some(1)
                        && n.sc == Some(0)
                        && n.frequency == Some(400_000)
                        && n.divider == Some(1)
                        && n.names == b"main\0dma\0"
                        && n.open_drain
                        && n.irq_sel
                        && n.rs_stop,
                )),
                (2, "aw87xxx_smartpa_0@58") if stack[1].name == "i2c@11004000" => Some((
                    4,
                    has(n.compatible, b"awinic,aw87xxx_pa")
                        && word(n.reg) == Some(0x58)
                        && n.index == Some(0),
                )),
                (2, "aw87xxx_smartpa_1@59") if stack[1].name == "i2c@11004000" => Some((
                    5,
                    has(n.compatible, b"awinic,aw87xxx_pa")
                        && word(n.reg) == Some(0x59)
                        && n.index == Some(1),
                )),
                _ => None,
            };
            if let Some((i, valid)) = candidate {
                seen[i] += 1;
                good[i] += u32::from(valid && !n.disabled && !n.unsupported);
                if i == 0 {
                    infra_handle = n.handle;
                }
                if i == 3 {
                    i2c_clocks = n.clocks;
                }
            }
        }
    })?;
    let mut uses = 0;
    device_tree::walk(blob, |e| {
        if let Event::Property("phandle", v) = e {
            if word(v) == Some(infra_handle) {
                uses += 1;
            }
        }
    })?;
    let expected = [infra_handle, 11, infra_handle, 38];
    let clocks = i2c_clocks.len() == 16
        && i2c_clocks
            .chunks_exact(4)
            .zip(expected)
            .all(|(v, w)| word(v) == Some(w));
    Ok(root
        && good == [1; 6]
        && seen == [1; 6]
        && clocks
        && uses == 1
        && infra_handle != 0
        && infra_handle != u32::MAX)
}
