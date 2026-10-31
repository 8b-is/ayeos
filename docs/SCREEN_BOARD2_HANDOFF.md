# Board2 targeted screen diagnostic

Author: OpenAI GPT-6 Astra (`openai/gpt-6-astra`), 2026-10-31.
Family context v2 / seed `8b-is-family-seed-v1`.
Base Board1 commit: `d05c93907191504f52c651d395d38ea2351ddfd8`.
Worktree: `/data/.agent-workspaces/ayeos-717a3e93d986-phone-bringup-20261031`.

Board1 photos established diagnostic execution and structural FDT parse on the
handset. Its broad PMIC ancestor match consumed the first 32 records before
audio/connectivity. Board2 replaces that selection with four bounded groups and
repairs the missing `@` and `~` glyphs. See PHONE_BRINGUP.md for selection limits.

## Artifact

Server directory: `/data/os-verification-20261031/screen-board2/`

- Image: `studio-board2.img`, 24576 bytes.
- SHA-256: `614d6207401edda9c5f4e40bbf0637e6329d0e250f50f2a5c5f8903327175111`
- Raw kernel: 45168 bytes, SHA-256
  `133895e6fab79d0a80151f90583ec14337d108b76b8cd381dc61a03f66afe6eb`.
- Receipt: `receipt.json`. Hardware verification remains pending for Board2.

Eight host tests pass, including PMIC-child starvation, independent selection,
property-order-independent amplifier detection, clock lookup, malformed blobs,
text bounds and omission accounting. Release build passes from `baremetal/`;
an initial build from repository root failed because Cargo did not load the
nested linker configuration. Inherited unused/unreachable warnings remain.

## Capture request

After coordinated deployment, photograph the full stationary screen, including
all four section headings and the final overflow/omission counters. Confirm
`studio-board2` in the banner and visible `@`/`~` symbols. Zero matched records
are useful evidence about the selector, not proof that hardware is absent.
No USB polling, page-button handling, or peripheral initialization is added.
Inherited boot-memory/framebuffer assumptions remain as documented for Board1.
Use the locally reviewed deployment procedure; do not use the old updater that
erases both init_boot slots. Preserve Board1's image and original photos.
