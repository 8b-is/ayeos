# Explicit phone communication and loader roadmap

User direction (2026-10-31): standalone AyeOS, with communication under the
owner's control, eventually able to disconnect USB and call the owner's service.
Author: OpenAI GPT-6 Astra. This is a design direction, not deployed capability.

## Communication contract to implement

- Owner-configured destination and provisioned server identity; no discovery of
  arbitrary cloud services, advertising, unsolicited telemetry or cloud fallback.
- Visible connection state and explicit pairing. A service must mediate access;
  this coding chat is not itself a persistent endpoint or phone controller.
- Small typed commands with bounded sizes, deadlines, sequence/replay checks and
  acknowledgements. No arbitrary shell/eval channel. A local physical disconnect
  control remains available independently of the service.
- Distinguish read-only status, device actions and firmware installation. Use
  genuine cryptographic authentication for commands and updates; the historical
  public-hash attestation is not publisher authentication.
- Preserve recovery and an independently verified known-good image. Validate
  interrupted downloads, rejected signatures, power loss and failed boots before
  enabling remote installation. Keys and private data stay out of public reports.

These are planned mechanisms. Board3 only displays allowlisted FDT resources and
reads the keypad; it does not connect to any service or enable a radio.

## Implementation order

1. Complete board-resource capture; verify clocks, resets, IRQs, memory windows
   and the actual chipset/ROM. Resource declarations alone prove no bus access.
2. Implement bounded CONNSYS power/transport and observe a real command/event
   exchange. Identify required firmware files and their provenance/licensing.
3. Bring up WLAN client mode and verify association, packet transmission and
   reception before adding addressing and an authenticated application channel.
4. Deploy a dedicated owner service and the explicit paired phone protocol.
5. Add a small AyeOS **second-stage loader** launched by the existing factory
   bootloader. This can eventually verify/select AyeOS images without Linux.

Replacing the factory LK/preloader is a later, different project: BootROM and
secure-boot requirements, DRAM initialization, storage, display and reliable
recovery must be established first. Unlocking kernel boot does not establish that
every earlier boot stage accepts replacement images. No bootloader partition
writes are part of the current diagnostic work. Vendor chip firmware and secure
monitor dependencies must remain documented rather than called replaced.
