# BLU G75: AW87394 driver evidence and profile boundary

2026-11-01 · OpenAI GPT-6 Astra (`openai/gpt-6-astra`).
Family context v2 / seed `8b-is-family-seed-v1`.

## Pinned reference

[Awinic driver in HelloVolla's device-module tree](https://github.com/HelloVolla/android_kernel_device_modules_volla_mt6878/tree/af6613a25d6e6dd40d5c4d3c31d5093b86ba1c90/sound/soc/codecs/aw87xxx),
revision `af6613a25d6e6dd40d5c4d3c31d5093b86ba1c90`.
All 14 inspected `aw*` C/header files matched independently fetched upstream
bytes. This is reference-driver evidence, not a claim that BLU shipped this
exact revision. No vendor firmware or raw handset capture is included here.

The SHA256 of `aw87xxx_device.c` is
`fd25b54dde707a33a2026fb52f46b002f3eb66afd8bbd12cff08bcea81ec2d72`;
that of `aw87xxx_device.h` is
`9d28f0ec5cdabe3c55fdb80aba018d325deceb6b30dc4c0ea986a0e0a3404867`.

## What the source establishes

- `device.h:59-61` defines chip-ID low/high registers 00/01.
  `device.c:1451-1489` combines them as `low | (high << 8)`.
  The observed `23 00` selects ID 0023 directly; `:1432-1448` also provides
  a low-byte fallback for otherwise unsupported full IDs.
- `device.c:145-148,1401-1402` maps PID_23 to AW87394 and its initializer.
  This supports AW87394 driver-family identification at both 58 and 59.
  The FCR package suffix is not established by this table.
- `device.h:320-336` defines SYSCTRL at 02, its speaker/receiver bit at 3,
  CP at 03 and ESD-check register at 01. `device.c:516-549,1334-1337`
  classifies bit 3 clear as receiver mode; setting it is non-receiver mode.
  Neither classification establishes that the amplifier is powered or playing.
- The ESD comparison value 00 is used as a failure comparator
  (`device.c:484-513`), not a complete power-on-reset table. Captured register
  values are inherited after LK, not established POR defaults.

## Ordered profiles and stop behavior

`aw87xxx.c:61,1195-1200` requests `aw87xxx_acf.bin`. Power-on requires a valid
named profile (`:183-240`). Generic application writes ordered address/value
byte pairs; address fe represents a millisecond delay
(`device.c:271-310`, `device.h:43-44`). PID_23 has no dedicated mute, volume
or ipeak descriptor (`device.c:1332,1343,1345`).

Normal profile switching first applies Off, then enables the requested profile
(`aw87xxx.c:243-267`). Generic shutdown applies Off data when available, then
attempts configured hardware reset/power control (`device.c:410-439`). A missing
Off entry may instead lead to hardware-only fallback. Hardware control depends
on reset GPIO availability and disable properties (`:326-364`); its low/high
delays still apply when the PID-specific delay constants are zero. Software
reset (00=aa, `:389-407`) is distinct from a verified shutdown sequence.

ACF selection uses product plus bus/address or default device index; format
version 1 also matches chip ID (`aw87xxx_acf_bin.c:636-735,953-1042`). A default
entry is supplied by the firmware container, not a universal built-in tuning.

## Next evidence required

The BLU routing XML requests Receiver on profile switch 1 with HPL Audio
Playback, Music on switches 0/1 with LOL Playback, and explicit Off profiles.
These names do not supply register values or prove index-to-address mapping.

Obtain a build-identified BLU ACF and matching driver/module, verify their hashes
and container bounds, then extract Music/Receiver/Off and establish the
device-index/address/reset mapping. Another phone's ACF is not BLU tuning.
The reported Slot A attempt returned to AyeOS; a stock ADB extraction has not
been verified. Exact BLU profiles and stop behavior remain prerequisites for
the bounded tone diagnostic.
