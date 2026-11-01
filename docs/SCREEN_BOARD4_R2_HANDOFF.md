# Board4 r2: USB/TPHY binding evidence and fallback visibility

OpenAI GPT-6 Astra (`openai/gpt-6-astra`), 2026-10-31.
Family context v2 / seed `8b-is-family-seed-v1`.
Base: Board4 `4a6f1916e328b87ed4278a9ffe420291e41fd0d8`.

Downloads: <https://g8t.is/downloads/ayeos/studio-board4/r2/>.
The original Board4 image remains at its original URL. r2 has a distinctive
`studio-board4-r2` screen banner, while retaining the same EP0 protocol and
capture-script compatibility. See [the original handoff](SCREEN_BOARD4_HANDOFF.md)
for the read-only transport, hardware assumptions, source license and build steps.
The adjacent receipt identifies this revision's image and source hashes.

## Evidence checked

The archived BLU files in AyeOS revision
`e432c24abe817f89bcd9e81f72017e6ea0a058a9` confirm:

- `hardware/blu_g75_g1170/loaded_modules_proc.txt`: `musb_hdrc`, `musb_main`,
  `phy_mtk_tphy`, `phy_generic`, `extcon_mtk_usb`, `tcpc_sc2150`.
- `hardware/blu_g75_g1170/getprop_full.txt`: `ro.kernel.version=6.6` and
  `sys.usb.controller=musb-hdrc`.
- `hardware/blu_g75_g1170/vendor_init/init/hw/init.mt6768.usb.rc`: the same
  controller name and `/sys/devices/platform/mt_usb` controls.

These are consistent with the target's MediaTek USB subsystem. A loaded module
name and a sysfs platform name do not determine an FDT compatible string.
The archived kernel-version property is not a fresh observation of a running OS.

Additional pinned reference:
[OnePlusOSS MediaTek device modules 6.6](https://github.com/OnePlusOSS/android_kernel_modules_and_devicetree_oneplus_mt6991/tree/76d6e1e5b8deb0bcd00355d7a9e68638a501d8bc/kernel/kernel_device_modules-6.6).
This is a reference tree, not a claim that it is the BLU vendor's exact source.

- `arch/arm64/boot/dts/mediatek/mt6768.dts`, lines 3483–3533, declares
  `mediatek,mt6768-usb20`, controller `0x11200000`, shared region `0x11cc0000`,
  `usb_phy_offset=<0x800>`, `mediatek,generic-tphy-v1`, and port `0x11cc0800`.
  Clock names differ from the older 4.19 reference, but Board4 does not require
  the old clock names. Both reference layouts match the existing profile.
- `drivers/misc/mediatek/usb/usb20/musb_main/musb.c`, lines 229–303, binds a
  platform driver named `musb-hdrc`; it provides no OF compatible alias table.
- `drivers/phy/mediatek/phy-mtk-tphy.c`, lines 2075–2113, sets the USB2 COM bank
  to `port_base+0x000` for v1 and **`port_base+0x300` for v2**. Its match table
  at lines 2949–2955 lists generic v1/v2/v3 and several other SoC variants, but
  does not establish `mediatek,mt6768-tphy` or `mediatek,mt6768-phy` as v1 aliases.

Therefore r2 recognizes all the requested names for **diagnostic display**.
It does not reinterpret v2 or an unverified alias as the v1 hardware layout.
A compatible list containing an explicit supported fallback is accepted with
the remaining profile checks; contradictory v1/v2/v3 declarations are rejected.
An optional `usb_phy_offset` must equal `0x800` when present. The linked port,
SoC, translation, addresses and clock-provider checks remain required.

## What r2 improves

- Whole-node discovery makes fallback output independent of property order.
  `reg` preceding `compatible` is retained even on an unfamiliar node name.
- Names/compatibles containing USB, MUSB, TPHY, U2PHY or `mt6768-phy`, their PHY
  children, vusb regulators and known-address candidates are shown. Candidate
  recognition alone never enables MMIO.
- Core controller/PHY identity fields are printed before supply/helper identity
  and auxiliary fields, so early USB helper nodes cannot consume the whole page.
- Controller `reg` values up to 32 bytes are displayed, including both regions;
  `usb_phy_offset` is included. `~` marks shortened data or paths.
- Screen counters distinguish observed v1, v2 and other requested bindings.
- Capacity is 32 candidate nodes and 24 displayed two-line records. The screen
  reports omitted records and lost candidates separately. It remains a bounded
  fallback, not a full FDT substitute.

Build: `cargo build --release --locked --features usb-report` from `baremetal/`.
Run `baremetal/tests/usb_board4.rs` with host rustc as in the original handoff.
The suite now covers the 6.6 reference shape, conflicting offsets/layouts,
supported compatible-list fallbacks, every requested alias in display-only
mode with zero initialization accesses, late compatible properties and candidate
overflow prioritization. Capture/packaging tests are unchanged.

Next handset evidence: a successful private USB FDT capture, or one whole-screen
r2 photo plus host USB errors. If an unsupported actual binding appears, use its
exact path, compatible list, reg, phys and PHY layout to implement that profile.
No handset enumeration, PMIC rail readiness, audio/FM/Wi-Fi or TCPC operation is
established by the module list or this build.
