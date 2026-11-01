//! Additional fixed tone profile: direct-root ARM timer and always-on VAUD28.
//! Existing I2C/PWRAP matchers independently validate SoC/clients/reg windows.
use crate::device_tree::{self, Event};

#[derive(Clone, Copy, Default)]
struct Node<'a> {
    name: &'a str,
    compatible: &'a [u8],
    hz: Option<u32>,
    min: Option<u32>,
    max: Option<u32>,
    regulator: &'a [u8],
    always: bool,
    disabled: bool,
    unsupported: bool,
}
fn word(v: &[u8]) -> Option<u32> {
    Some(u32::from_be_bytes(v.try_into().ok()?))
}
pub fn matches(blob: &[u8]) -> Result<bool, device_tree::Error> {
    let mut stack = [Node::default(); 32];
    let mut depth = 0;
    let mut seen = [0; 3];
    let mut good = [0; 3];
    device_tree::walk(blob, |e| match e {
        Event::Begin(name) => {
            stack[depth] = Node {
                name,
                disabled: depth > 0 && stack[depth - 1].disabled,
                unsupported: depth > 0 && stack[depth - 1].unsupported,
                ..Node::default()
            };
            depth += 1;
        }
        Event::Property(name, v) => {
            let n = &mut stack[depth - 1];
            match name {
                "compatible" => n.compatible = v,
                "clock-frequency" => n.hz = word(v),
                "regulator-name" => n.regulator = v,
                "regulator-min-microvolt" => n.min = word(v),
                "regulator-max-microvolt" => n.max = word(v),
                "regulator-always-on" => n.always = v.is_empty(),
                "status" => n.disabled |= v != b"okay\0" && v != b"ok\0",
                "ranges" | "power-domains" => n.unsupported = true,
                _ => {}
            }
        }
        Event::End => {
            depth -= 1;
            let n = stack[depth];
            let pmic = stack[1].name == "pwrap@1000d000" && stack[2].name == "mt6358-pmic";
            let candidate = match (depth, n.name) {
                (1, "timer") => Some((
                    0,
                    n.compatible == b"arm,armv8-timer\0" && n.hz == Some(13_000_000),
                )),
                (4, "ldo_vaud28") if pmic && stack[3].name == "mt6358regulator" => Some((
                    1,
                    n.regulator == b"vaud28\0"
                        && n.min == Some(2_800_000)
                        && n.max == n.min
                        && n.always,
                )),
                (3, "mt6358codec") if pmic => Some((2, n.compatible == b"mediatek,mt6358-sound\0")),
                _ => None,
            };
            if let Some((i, valid)) = candidate {
                seen[i] += 1;
                good[i] += u32::from(valid && !n.disabled && !n.unsupported);
            }
        }
    })?;
    Ok(seen == [1; 3] && good == [1; 3])
}
