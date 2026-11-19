# Session record

## 2026-11-19 — research reconciliation

Reviewed existing specifications, independent critique, handoff, implementation paths, tests and focus UI. Preserved the existing prototype. Updated the contributor guide and initialized the missing state files. M1 is proposed for approval with measurable exit criteria.

- A direct synthetic native-message frame on an empty temporary SQLite database returned success with session inactive. Database counts were zero studies and one page. This verifies a storage boundary gap independently of Chromium.
- Source inspection showed that the default context query has no session end bound and practice can append prior titles and retry details after the consent prompt.
- The standard pilot command passed 10/13. Focus duration and page counts were duplicated and the pilot assignment count differed. The test helper reuses a directory named by process ID and test. With a fresh temp root, all 13 passed. Reproduce the isolated baseline with `TMPDIR="$(mktemp -d)" mise exec -- cargo test --locked --offline -p whiteroom-core --test pilot`.
- The full platform/sandbox suite, visual checks and live provider were outside this research check. No application source was changed.
- Git reported no repository. The mounted `.git` directory is empty and read-only. The files are present in the workspace. A commit against original history requires a checkout with its writable metadata.
- Next authorized action after M1 approval is the test-fixture isolation repair. Check D3 and D4 at session start.
- Independent reviewer confirmed the collection, query and preview findings. It also identified premature feedback on interrupted day-seven blocks, missing first-presentation records and overwritten item forms. These are pilot acceptance dependencies in the architecture review and backlog.

Prepared the local supervisor launcher and ignored its runtime logs. Readiness check stopped at the existing approval gate and unavailable Git metadata. The launcher validation command runs the pilot suite in a fresh temporary directory.

## 2026-11-19 — approved M1 implementation

Owner approved D4. Window and browser consent are per-session, separately selectable at start and revocable in the desktop. Session IDs constrain tracking, native writes and default summaries. Existing rows survive migration. Unassigned historical activity remains accessible in explicit time-range queries and exports. An independent reviewer found navigation, upgrade and session-switch races. The final changes validate the document origin before reading the DOM, reject legacy SQL writes at the database boundary, read active controls in one snapshot and bind stop summaries to the stopped study.

Offline workspace build passed. The 16-test pilot suite passed twice in the same parent temp directory. The native-host test exercised the actual CLI subprocess. The recording-transport test verified the preview for initial and retry requests and zero calls on refusal. The browser-worker fixture passed permission, navigation and disconnect cases. No provider calls were made. Reproduce with `./scripts/supervise check`.

The supported desktop remains unavailable (GTK display initialization failed, local Broadway socket creation denied). Sandbox preflight returned a read-only filesystem error. Git metadata is unavailable. Claude Code's desktop assignment is in `docs/prompt-claude-m1.md`. Resume with WebKitGTK keyboard/layout verification and checkpoint in the original checkout.

The owner supplied the repository URL https://github.com/21usual7/whiteroom. A read-only Git lookup could not resolve github.com from this environment. Remote HEAD and ancestry remain unverified.

Reviewed Claude’s first M1 desktop report. Real 900 px session controls worked and the updated theme fit both offscreen widths. Fixed focus restoration on revocation and rebuilt the current UI. JS syntax and supervisor checks passed. Final real-window keyboard and layout verification and Git checkpoint are pending.

SSH lookup of git@github.com:21usual7/whiteroom.git stopped before authentication: OpenSSH rejected a system include owned by nobody in this environment. Host lookup for github.com and ssh.github.com also returned no addresses. Host-side SSH history handoff is documented in docs/prompt-claude-git.md.

Owner corrected the published repository to https://github.com/21usual7/whiteroomfocus. Local Git metadata is now readable at master, 2ef31a0. Remote synchronization was not verified.

## Supervisor orchestration

The owner requested a repository-local supervisor. The initial M1 queue preserves the existing desktop acceptance and owner checkpoint gates. Product implementation and research remain outside this change.

All 16 supervisor tests passed using mocked Codex and temporary Git repositories. The real offline bubblewrap suite passed Cargo check, Clippy, one CLI unit test, 16 pilot tests, JavaScript syntax and the browser consent fixture. Clippy reported four existing style warnings. No live Codex calls were made. The supervisor changes are available in the existing checkout for owner review; the checkout still contains pre-existing uncommitted files. M1 desktop acceptance remains pending.

## 2026-11-19 — M1 closure

The owner accepted the real-window WebKitGTK checks and local Git checkpoint `31b699c` in D6. Keyboard navigation, the focus ring after arrow selection and focus restoration after each revocation passed at 360×640 and 900×640. The current UI was unchanged from the reviewed UI. Both M1 items are in the completed ledger. The supervisor queue is empty while the next milestone awaits owner approval.

All 17 supervisor tests passed. The launcher reported M1 closed, returned code 3 for `run` and `--status`, and made zero Codex calls.

## 2026-11-19 — proposed follow-up milestones

The backlog now records ordered tasks, acceptance evidence and approval boundaries for browser blocking recovery, local data retention and deletion, the delayed assessment pilot and pilot-driven learning workflow improvements. The active supervisor queue awaits owner approval.
