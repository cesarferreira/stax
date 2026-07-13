# Codex-Inspired Stax GUI Design QA

## Evidence

- Source visual truth: `/var/folders/lb/g35_rg7j1xdc51ncphzlt21w0000gn/T/codex-clipboard-45ae3713-2655-43af-96ad-4c1876a4e8f6.png`
- Topology visual truth: `/var/folders/lb/g35_rg7j1xdc51ncphzlt21w0000gn/T/codex-clipboard-a08b6ac4-f000-489c-a6b8-f53f82c5afae.png`
- Final implementation screenshot: `/var/folders/lb/g35_rg7j1xdc51ncphzlt21w0000gn/T/com.openai.sky.CUAService/1/Stax QA 4 Screenshot 2026-07-13 at 6.42.24 PM.jpeg`
- Normalized full-view comparison: `/tmp/codex-stax-comparison-final.png`
- Reference viewport after removing the surrounding black canvas: 1792 × 1180, normalized to 1144 × 753.
- Implementation viewport: 1100 × 753.
- State: dark appearance, repository `/Users/cesarferreira/code/github/stax`, current branch `cesar/gpui-gui-codex-redesign`, all three panes visible.

## Full-View Comparison

The normalized side-by-side comparison shows the same major composition as the
reference: a graphite navigation/sidebar surface, deep navy primary canvas,
compact top bar, low-contrast separators, soft selected-row treatment, and an
inset raised inspector card. Stax intentionally preserves its product-specific
three-pane workflow instead of copying Codex's task navigation or conversation
content.

The branch gutter follows the supplied `st ls` evidence. Lane positions, nested
vertical rails, current-node treatment, side-lane return corners, and the trunk
join remain aligned while branch names start at a consistent column.

## Required Fidelity Surfaces

- **Fonts and typography:** The interface uses the macOS system UI font for
  chrome and Menlo for branches and diffs. Weight hierarchy, single-line branch
  identity, compact labels, truncation, and readable diff line spacing are
  consistent with the reference's restrained desktop typography.
- **Spacing and layout rhythm:** The 50 px toolbar, 44–46 px pane headings,
  48 px two-line branch rows, 8–16 px insets, rounded selected rows, rounded
  file summary, and rounded inspector card create the same compact-but-calm
  rhythm. No persistent control or pane content overflows the viewport.
- **Colors and visual tokens:** The final dark palette uses a deep navy canvas,
  graphite sidebar, quiet blue-gray raised surfaces, muted borders, lavender
  focus/accent, and accessible semantic colors. Automated contrast checks pass
  for focus and small status text. Cyan/green/lime topology lanes retain Stax's
  CLI identity without overpowering branch text.
- **Image quality and asset fidelity:** The Stax workspace contains no required
  raster imagery, illustration, avatar, or product photo. Codex-specific logos
  and navigation icons were intentionally not copied; no placeholder image,
  handcrafted SVG, or generated approximation was introduced.
- **Copy and content:** Existing Stax repository, stack, branch, PR, CI, diff,
  and action copy remains intact. Codex-specific task and chat copy was not
  transplanted into the product.

## Focused Evidence

A separate crop was not required: the normalized 2244 × 753 comparison keeps
the stack topology, branch metadata, diff text, toolbar controls, and inspector
typography readable at native implementation height. The same final native app
was also exercised by selecting `cesar/gpui-gui-diff-cache` and returning to
`cesar/gpui-gui-codex-redesign`; selection, diff content, inspector content,
and the immediate cached return all updated correctly.

## Comparison History

### Iteration 1

- Evidence: `/tmp/codex-stax-comparison-pass-1.png`.
- [P2] The long inspector branch name wrapped after `cesar`, unlike the
  reference's compact single-line identity.
- [P2] The application-wide focus border was materially brighter than the
  reference's quiet window frame.
- Fixes: removed the window-level focus-colored border and attempted a
  single-line truncated inspector identity.

### Iteration 2

- Evidence: `/var/folders/lb/g35_rg7j1xdc51ncphzlt21w0000gn/T/com.openai.sky.CUAService/Stax QA 2 Screenshot 2026-07-13 at 6.40.09 PM.jpeg`.
- [P2] The first truncation treatment collapsed the branch and parent identity
  to ellipses because the nested flex item did not receive a usable text width.
- Fix: supplied explicit full-width/min-width constraints to the card content.

### Iteration 3

- Evidence: `/var/folders/lb/g35_rg7j1xdc51ncphzlt21w0000gn/T/com.openai.sky.CUAService/Stax QA 3 Screenshot 2026-07-13 at 6.41.38 PM.jpeg`.
- [P2] Ellipsis-only output remained, so the truncation strategy was unsuitable
  for this GPUI hierarchy.
- Fix: removed truncation and used the smaller single-line monospace treatment
  that fits the inspector's supported width.

### Final Iteration

- Evidence: `/tmp/codex-stax-comparison-final.png`.
- The full branch and parent identities render on one line, the frame is quiet,
  and no actionable P0, P1, or P2 mismatch remains.

## Findings

No actionable P0, P1, or P2 findings remain.

## Follow-up Polish

- [P3] A future icon-system pass could replace some text-heavy toolbar controls
  with native symbols, but that requires a separately selected icon language
  and is not necessary for this reference-driven redesign.

## Implementation Checklist

- [x] Codex-inspired dark semantic palette.
- [x] Compact toolbar and quiet dividers.
- [x] Rounded stack selection and search surface.
- [x] `st ls`-equivalent topology lanes and joins.
- [x] Raised file summary and inspector card.
- [x] Keyboard, pointer, loading, and cached branch-return behavior retained.
- [x] P0–P2 visual findings fixed and recaptured.

final result: passed
