# WHITE ROOM architecture

The product uses study activity to support retrieval practice and delayed assessment. The first user-requested section is cybersecurity. The six focus modes label sessions and offer distinct directions. Evidence of learning will require unseen follow-ups with denominators and recorded delays. [Product specification](research/product-specification.md) · [Pilot critique](research/independent-review.md)

## Existing prototype

| Component | Responsibility and boundary |
| --- | --- |
| Rust core and SQLite | Shared learning records, FSRS, focus catalog and session history. CLI and desktop open the same database. Existing sessions without focus metadata remain readable. |
| CLI and desktop | The CLI owns tracking, native messaging and cloud requests. Tauri exposes explicit commands to a local HTML/CSS/JS interface. The desktop uses the sibling CLI for the tracker. |
| Activity sources | Hyprland reports focused windows, Wayland reports idle state. Chromium reports active tabs via native messaging and applies its own blocking rules. Browser blocking and data collection have independent lifecycles. |
| Execution boundary | The existing challenge runner uses bubblewrap and systemd user limits. Its acceptance suite covers the isolation contract. Keep this boundary intact. [Technical feasibility §8.1](research/technical-feasibility.md) |
| Data leaving the device | CLI review and practice construct provider requests. Both display the outgoing JSON. Practice includes prior titles and can include retry output, each displayed in the confirmed request body. |

## Pre-M1 snapshot against requirements

P1–P3 specify separate source opt-ins, visible collection state and disclosure before sending. The extension listens for tab changes independently of session state. Its study-site list controls excerpt extraction. The native host validates and persists page messages without consulting an active study or a source preference. A synthetic message on an empty database produced one page row with zero study rows. Window tracking starts with a session. The default AI activity query uses a lower time bound and can include activity after the selected session ended. [Extension](../extension/background.js) · [Storage](../whiteroom-core/src/lib.rs) · [CLI](../whiteroom-core/src/main.rs)

The pilot has held-out day-seven items after an objective is marked done. Generated review questions enter FSRS. Independent 1/7/30-day measurement for generated material remains planned. Use separate held-out forms or objective cohorts, record first unaided response and actual delay, and report missed follow-ups separately. Earlier testing of the same objective can affect later performance. [Cognitive engine §6](research/cognitive-engine.md) · [Independent review](research/independent-review.md)

## M1 implementation

Session creation stores explicit, per-session window and browser consent in SQLite. Both default off, including on upgrade. Revocation is immediate at the write boundary. Inserts carry the study ID. SQLite triggers also reject legacy writes without an authorized study. Window workers are bound to a session ID. The browser asks the host for current consent before querying a tab. Chromium enforces origin permissions and the extension checks the marked site before extracting text. It sends a one-use site grant on the same port before an excerpt. The host consumes that grant and rechecks consent inside the write transaction.

Default cloud context is limited by both study ID and time. Historical rows without a study ID remain in exports and explicit hour-range queries. Each request, including retries, goes through one confirmation function that displays the outgoing JSON before accessing the provider key. Tests inject a recording transport. See the [execution plan](../tasks/current.md).

CLI review and math practice extract caller-selected UTF-8 text attachments locally. Prepared contexts keep source labels, remove duplicate fragments and prioritize terms from the session topic. Both paths cap the full serialized request and set separate response limits. The preview shows the bounded JSON, estimated token count and number of omitted fragments.

## Browser blocking recovery

The extension reconciles installed dynamic rules after confirmed host state and retries failed updates on later confirmed replies. During loss of host confirmation, the popup can explicitly release WHITE ROOM rules through the extension. It reports the installed rule count after the change. Reconnection reconciles with the current session again. Browser capture still uses its existing per-session and per-site permissions.

During a focus session, the native host sends the session denylist (D18). One Chromium rule blocks page and frame navigations to those domains and their subdomains; other sites stay open. An empty list installs no rule. The extension also recognises the former allowlist rule as its own and replaces it. The denylist uses the `block` SQLite table. The per-topic `topic_site` table from D12 stays available in exports.

On Hyprland, a Quickshell Layer Shell surface shows elapsed time in the overlay layer. The CLI watches the active session ID in SQLite and the overlay closes when that ID stops being active.

## Theory tutor

The Rust tutor answers catalog questions using vetted definitions, comparisons and neutral examples. During a focus session it suggests a concept from the selected mode and direction. Outside a session, other questions may be mapped by Claude to a concept and format after the user confirms the exact JSON. Both desktop and overlay display the selected local lesson. Drafts are kept in memory, expire after two minutes and are bound to the session state.

The Tauri chat and the Quickshell overlay share the tutor's SQLite history for the current local day. The Wayland popup communicates with the Rust tutor over standard input and output. It opens from the timer during a session, or from the CLI, and closes when the session scope changes.

## Assessment integrity review

The independent source review found that exiting the CLI test early displays feedback for answered items and can present an already displayed unanswered item again. Before using results to claim first-exposure retention, persist first presentation, mark partial blocks and defer feedback until closure. Updating a module can replace item content under the same ID. Freeze reconstructible forms and keep the full personal export, including answer keys, separate from pilot analysis. [Test flow and item storage](../whiteroom-core/src/lib.rs)

## WHITE ROOM 1.0

`whiteroom-core/src/learn.rs` owns goals, plans and learning blocks; CLI (`today`, `goal`, `learn`, `progress`, `seen`, `backup`, `restore`) and Tauri commands call the same functions and receive JSON. Schema version 2 is an additive migration with a `VACUUM INTO` copy beforehand; a newer schema is refused. A block is one row with its plan; each step is answered at most once (`block_step` primary key) and at most one block is open, so repeated clicks and concurrent CLI and GUI clients converge on the same attempt. Hints are stored in the block, so a later answer from any client is marked as helped. Item edits create versions in `item_version`; attempts keep `item_v`. Delayed checks use the objective's `horizon` (7 or 30 days). Expressions and code are graded in the existing sandbox; self-graded cards and explanations never enter retention or first-attempt metrics. The native-messaging reply carries `protocol` and `version`; on a mismatch the extension reports it and leaves rules in place (D8). [Release report](release-1.0.md)
