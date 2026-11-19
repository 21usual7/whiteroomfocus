# M1 closed

The owner confirmed M1 closure on 2026-11-19 in D6. The two remaining acceptance items are complete. The next milestone awaits approval.

## Completion evidence

- Keyboard navigation, the focus ring after arrow selection, and focus after revoking each source passed in real WebKitGTK windows at 360×640 and 900×640. [Desktop report](../docs/claude-m1-review.md#повторная-проверка-после-пересборки-codex)
- The owner accepted local Git checkpoint `31b699c`. The checked-out UI matched the reviewed UI when the checkpoint was confirmed.

## Acceptance criteria

1. Stabilize the pilot test database fixture. Two runs with the same parent temp directory must start empty and pass independently.
2. Add separately visible source controls, default off for fresh and upgraded installs. With capture disabled the focus timer, mode selection and session history must still work. Preserve existing records.
3. Enforce active-session consent at collection and at the native write boundary. Use two sessions, a marked and an unmarked domain, and synthetic events before, during and after each session. Expect zero writes while inactive or disabled, metadata only with browser consent, excerpts only with site permission. Exercise stop, revoke and stale/disconnected browser state.
4. Bound default review data to the selected session. Confirm that historical or later activity cannot silently enter the preview. Compare every user-derived field of a recorded outbound request with its preview, including prior titles and retry details. Declining must yield zero calls. Use fixtures throughout.
5. Run regression checks for existing sessions, export, tracking and site blocking. Verify keyboard operation and narrow layout with a temporary profile on the supported desktop. Record any unavailable environments explicitly.

The milestone closed with passing behavior checks, independent desktop review, the documented data flow and a local Git checkpoint. Wider retention and deletion policy and paid provider validation have separate approval boundaries.
