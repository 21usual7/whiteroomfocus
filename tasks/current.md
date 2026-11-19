# Current milestone

The owner approved M1 on 2026-11-19.

## Resume here

Implementation and fixture validation are complete. The remaining approved work is desktop acceptance and the local Git checkpoint. Use the existing [Claude Code assignment](../docs/prompt-claude-m1.md) for the 360/900 px WebKitGTK checks. Preserve its ownership of theme.css. Close M1 after recording observed results on a supported desktop and checkpointing the reviewed changes in an accessible checkout.

The [first desktop report](../docs/claude-m1-review.md) covers the real 900 px session lifecycle and offscreen layout at both widths. Codex rebuilt the corrected styles and restored keyboard focus after source revocation. The Claude assignment now specifies the remaining real-window checks.

## Acceptance criteria

1. Stabilize the pilot test database fixture. Two runs with the same parent temp directory must start empty and pass independently.
2. Add separately visible source controls, default off for fresh and upgraded installs. With capture disabled the focus timer, mode selection and session history must still work. Preserve existing records.
3. Enforce active-session consent at collection and at the native write boundary. Use two sessions, a marked and an unmarked domain, and synthetic events before, during and after each session. Expect zero writes while inactive or disabled, metadata only with browser consent, excerpts only with site permission. Exercise stop, revoke and stale/disconnected browser state.
4. Bound default review data to the selected session. Confirm that historical or later activity cannot silently enter the preview. Compare every user-derived field of a recorded outbound request with its preview, including prior titles and retry details. Declining must yield zero calls. Use fixtures throughout.
5. Run regression checks for existing sessions, export, tracking and site blocking. Verify keyboard operation and narrow layout with a temporary profile on the supported desktop. Record any unavailable environments explicitly.

Exit requires passing behavior checks, independent review, a documented data-flow update and a local Git checkpoint when writable repository metadata is available. Wider retention/deletion policy and paid provider validation have separate approval boundaries.
