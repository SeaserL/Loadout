# Fusion desktop redesign — design QA

final result: blocked

Blocking scope: actual Windows desktop window/tray lifecycle. Automated approval rejected launching the built binary twice with only `blocked by policy`; the user cannot launch it manually now. Browser checks do not establish desktop acceptance.

## Visual target and comparison

Magpie local revision fd80ed99c9a97a2f6f96ee62e36eba9eefb16036, masking-zh-light (1912×1038), routing-zh-light and usage-zh-light. Browser implementation captured at approximately1265×712. Both reference and implementation were opened together in the same comparison tool result. This is an adaptation to OmniRoute management sections, not a pixel clone: the reference graph is agent-to-provider masking; ours is model-to-account token attribution. Comparison concerns fine strands, neutral background, violet focus, central hub, readable hierarchy and metadata density. No fake agent joins or moving packets are added.

## Findings and fixes

- P1: 064 delivered only a webpage, not a desktop entry. Added separate Tauri binary/config/profile and double-click launcher; actual desktop validation remains BLOCKED.
- P2: first redraw had overly dense solid-looking bands. Reduced strand density and separated gray input-side strands from violet account-side strands; recaptured overview.png after correction.
- P2: sidebar overflow at a normal-height viewport. Tightened group/button spacing; final screenshot shows all navigation and bottom controls.
- P2: custom provider UUID overwhelmed quota and pool rows. Resolve provider display names and show model name with exact identifier retained in tooltip; code/typed build and quota screenshot verified.
- P2: tab navigation preserved scrolled position. Scroll resets on section changes; model/pricing/overview screenshots recaptured.
- P2: empty native budgets contain null schedule fields. Omit nulls and preserve all existing periods on save; final UI save succeeded and native regression checks other periods and key scope unchanged.
- P2: native quota percentages may be synthetic. Display gateway signal separately from sourced/dated quota cache; unknown balance remains unknown.
- P3: desktop-host title bar, tray icon and minimum-window rendering remain unobserved. No desktop pass claimed.

## Interaction checks

Native login; all 12 section headings; account name save; explicit model sync and readback; budget two-step save; pricing search and sync cancel; Radar missing-date/empty search; node selection/clear and input/output metric changes; request explanation dialog and close. Keyboard focus rings remain visible. All write operations used a disposable local fixture, not the real account pool.

## Evidence

`development/fusion/evidence/065-desktop/ui/overview.png`, flow-selected.png, budget.png, pricing.png, models.png, pools.png, radar-empty.png. Screenshots contain synthetic records only. Source reference images stay in the upstream checkout and are not shipped as application assets.

Browser visual/interaction checks passed within the stated scope. Overall desktop delivery remains blocked until the native window and launcher are actually exercised.
