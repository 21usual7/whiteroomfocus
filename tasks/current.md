# Stage 1 — Browser blocking recovery

The owner approved this stage on 2026-11-19 in D7. M1 completion remains recorded in the completed ledger.

## Current task

Prepare the popup markup and styles in extension/popup.html and extension/popup.css. The first supervisor task adds a separate status region and a keyboard-accessible native button. Keep the action hidden until the recovery handler is wired. The acceptance fixture checks the popup contract, and Reviewer checks its layout and keyboard focus.

## Acceptance criteria

1. Reconcile actual installed rules with a confirmed session after a worker restart. An update failure must allow the same desired state to be retried.
2. Clear WHITE ROOM rules at session end. With the local process unavailable, the user can release WHITE ROOM rules from the extension popup using the keyboard. Show the outcome after verifying the installed rules.
3. Preserve other rules and the M1 capture and consent behavior. Cover restart, failed update and disconnect with synthetic fixtures.
4. Verify recovery in Chromium with a temporary profile and record results and the local Git checkpoint.

## Owner gates

Settle the behavior on connection loss and review protected implementation changes after the popup task. The temporary Chromium profile is the final manual acceptance gate.

Before this plan was committed, a synthetic worker run reproduced three issues. A restarted worker skipped clearing an empty desired list, a repeated update after failure was skipped, and the disconnect left existing rules installed.
