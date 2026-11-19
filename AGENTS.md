# Repository Guidelines

## Project Structure

WHITE ROOM is a local study and focus tracker for Hyprland and Chromium. Both Cargo workspace members use the same SQLite database.

- `whiteroom-core/src/` contains shared session, scheduling, and storage logic. The CLI starts in `main.rs`. Edit `focus.rs` to change the cybersecurity mode catalog and its directions.
- `whiteroom-app/` contains the Tauri desktop app. Rust commands live in `src/main.rs`, static HTML, CSS, and JavaScript in `ui/`, and application icons in `icons/`.
- `extension/` contains the Chromium Manifest V3 extension. `modules/` holds TOML learning content, and `challenges/examples/` holds JSON challenge fixtures.

## Build, Test, and Run

Run these commands from the repository root. Mise selects the Rust toolchain from `mise.toml`.

| Command | Purpose |
| --- | --- |
| `mise exec -- cargo build --workspace` | Build both debug binaries. The desktop app uses the sibling CLI to start tracking. |
| `./target/debug/whiteroom-app` | Launch the desktop interface. |
| `mise exec -- cargo test -p whiteroom-core` | Run the core test suite. |
| `mise exec -- cargo test -p whiteroom-core --test pilot` | Check study sessions, scheduling, and browser messaging during development. |

## Coding Style

Use four spaces in Rust and two in HTML, CSS, and JavaScript. Follow Rust naming conventions such as `snake_case` functions and `UpperCamelCase` types. Use `camelCase` in JavaScript and keep user-facing text in Russian. Apply rustfmt to touched Rust code and keep formatting changes scoped. Keep business rules in the core crate. Register new Tauri commands in the invoke handler, `build.rs`, and `capabilities/main.json`.

## Testing Guidelines

Integration tests use Rust’s built-in test harness under `whiteroom-core/tests/`. Name tests after observable behavior, such as `eof_mid_session_is_safe`. Cover session persistence in `pilot.rs`, challenge validation in `challenge.rs`, and isolation guarantees in `sandbox.rs`. Sandbox tests require bubblewrap and a working systemd user session. Use a temporary `WHITEROOM_DB` for manual checks and fixture responses for Claude API tests.

## Commits and Pull Requests

Git history is unavailable in this checkout. Use a short imperative subject describing the change. In pull requests, explain the user-visible effect, link applicable issues, and list the checks run. Include a screenshot for visual changes and identify any desktop behavior that still needs verification.

## Session Workflow

Read [.agent/state.json](.agent/state.json), [progress](.agent/progress.md), and the current, backlog, and completed records under `tasks/` before selecting work. Use [architecture](docs/architecture.md) for component boundaries and [decisions](docs/decisions.md) for authorization. The handoff and research roadmap are dated evidence.

Select the highest-priority unblocked item within the approved milestone. Record its acceptance criteria in `tasks/current.md`. Verify observable behavior, obtain independent review where available, and record findings and limitations. Update the ledger and state at each checkpoint. Inspect staged changes for scope and secrets before committing locally. Record a blocked checkpoint when repository metadata is unavailable.

Handle routine reversible choices independently. Escalate product or privacy changes, irreversible actions, external changes and spending. Stop after three materially different attempts on a blocker and continue independent work. At an approval boundary, record the decision and options once. Resume from the recorded gate when the owner responds.

For a bounded local agent run, use `scripts/supervise --status` to check readiness. After explicit owner approval, record the scope in the decision log and set the state status to `approved_for_implementation`. Run `scripts/supervise` from a clean checkout. It uses the downloaded supervisor (override its path with `SUPERVISOR_PY`) for one task with a 20-minute budget. Its changes remain available for review before a checkpoint. Restore the pending status when the approved scope ends.

Place new Claude Code assignments beside the existing prompts in `docs/prompt-claude-*.md`. Define file ownership and acceptance checks in each assignment.
