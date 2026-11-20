# Learning MVP (D19, docs/prompt-claude-mvp.md)

Acceptance and results, 2026-11-20. Synthetic data, temporary `WHITEROOM_DB`, no paid calls.

- Weekly check: the original bug was reproduced on HEAD 8faafdb (review after one answer, a seen question shown as new, the briefing silent after the first answer). The fix records first exposure before display, resumes after EOF, window close and restart, freezes form and key, reveals keys only after the whole block closes and refuses duplicate answers from a second client. Tests: `weekly_*`, `pilot_report_matches_hand_count`, `review_session_is_refused_while_weekly_block_is_open` in `pilot.rs` (30/30).
- Pilot report and anonymous export: per-group counts with numerator and denominator, misses, late answers, reshows and actual delay. A hand-counted synthetic sample matches. A hinted answer is not independent and does not enter calibration. FSRS treats a hinted answer as a lapse (`Again`). `Hard` is not set yet because no difficulty signal exists.
- Claude questions: they go to review drafts first (edit, reject, accept, report an error after adding). Review previews bind to a finished session snapshot. The tutor, hints, keys and the Wayland popup are closed during a self-test block. Tests: `review.rs` 7/7.
- Real Tauri/WebKitGTK on an isolated Xwayland with a clean database: practice with a hint, a mistake and a retry; weekly check opened from the briefing; window killed after one answer; on reopen the block resumed at 2/3 with focus in the answer field; review only after closing; pilot report on the dashboard matches the CLI; setup editor saved a valid file; D17 preview of explicit text and cancellation staged nothing. Mixed keyboard and pointer at 900×640 and 360×640. `ANTHROPIC_API_KEY` was present in the process environment and no send was pressed.
- The stubbed-IPC harness covers the Claude fixture cases (empty result, disputed key, edit, reject, resend, cancel, session change) at 360 and 900 px: 41/41 each. These cases were not repeated in the real window because they need a provider response.
- Browser policy D18: fixtures rewritten for the denylist, 7/7. The temporary-profile Chromium check of the denylist has not been run.

# Tutor chat and Wayland popup

- Tauri/WebKitGTK and the native Wayland popup answered catalog questions with no provider credentials. The Tauri check verified the exact cloud preview and cancellation. The response is covered by a transport fixture and provider model access is verified.
- The native popup accepted typing over a fullscreen Wayland application and returned focus after Esc or session end. The local chat history was shared between the popup bridge and the Tauri dialog.
- An independent frontend run passed in WebKitGTK at 360×640 and 900×640 and in Chromium at 320, 360, 900 and 1280 px. The tested screens had no horizontal overflow.

## Topic allowlists and Wayland timer

- Passed in Chromium 155 with a temporary profile and SQLite database. Google and the current topic's domain load with capture off. Other hosts are blocked. Changing topics replaces the exceptions, and stopping removes the rules. Browser fixtures cover disconnected recovery. The default desktop profile now has the unpacked extension and registered native host.
- Passed in the Tauri/WebKitGTK window with a temporary database. Choosing a topic opens its site list, saving adds its domain, and starting shows the timer in Hyprland's overlay layer. Ending the session removes it. The CLI start check also verified that its process can exit while the timer remains visible.

## Previous checkpoints

All three tasks from the browser recovery queue are complete. The current interactive session carried out the work after the autonomous run reached its token limit. The original run and its usage remain in the supervisor history.

- Browser checkpoint: `b37c8b9`. The temporary Chromium profile verified keyboard recovery with the native host offline and preservation of an unrelated rule.
- Local context checkpoint: `14b40a3`. Review and math practice accept explicitly selected UTF-8 text and Markdown files. The full outgoing JSON is bounded before confirmation. Both paths were verified on a temporary database without an API key.

## Local data and tutor

- [Local data inventory](../docs/local-data-inventory.md) was checked with temporary SQLite and Chromium profiles. The owner decision on retention and deletion scope is next.
- The theory tutor backend and Tauri contract are available. Claude Code owns the navigation section and active-session dialog. [Frontend assignment](../docs/prompt-claude-tutor.md)
- A WebKitGTK build was exercised on an isolated Xwayland display. Keyboard navigation selected a mode and started a session at 900×640, and the same session stopped using the keyboard at 360×640. The background changed between frames. Native Wayland frame rate and the tutor UI still need acceptance.

See [completed work](completed.md) and the [backlog](backlog.md).
