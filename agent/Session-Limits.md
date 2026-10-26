---
type: "Reference"
title: "Session request and reported-token limits"
language: "en"
---

# Session request and reported-token limits

Set optional limits on an Agent session's model request attempts and service-reported total tokens. The runtime saves each attempt before dispatch, retains observations across restarts and explicit resumes, and pauses before the next model request or pending tool when a configured limit requires it. These are local execution guards, not provider quotas, context capacity or a monetary budget.

## Configure and inspect

Add a top-level table to the operator-selected configuration, outside provider and MCP tables. The following numbers are examples, not recommended project budgets:

```toml
[session_limits]
max_requests = 20
max_total_tokens = 50000
```

Both fields are optional. Omit a field to disable that guard; zero pauses a new session immediately. Negative values and unknown field names are rejected. Existing configurations without this table keep their previous behavior. The separate `max_steps` setting continues to bound one invocation of the Agent loop.

| Interface | Read local configuration and usage |
|---|---|
| TUI Chat | `/budget` |
| CLI, configuration only | `pm-agent --vault PATH budget` |
| CLI, saved session | `pm-agent --vault PATH budget --session ID` |

Queries do not contact a model, require a configured model/key, initialize MCP, or change session usage or history. The Chat command preserves the unsent composer draft. Configuration-only queries have no session observations. The active conversation's Context display continues to show only the most recent service-reported fields; session totals are displayed separately.

## What the guards measure

- A request attempt is recorded before calling the provider adapter. Failed authentication, protocol errors, cancellation and interrupted requests consume an attempt even when delivery to the service cannot be established. Counts are not claims about completed or billed requests.
- Each completed response retains its exact reported input, output and total fields in the private session ledger. Zero is retained. The session total sums only supplied `total_tokens` values; it never derives a missing total by adding input and output.
- The complete session total is unavailable if any attempt lacks a reported total, earlier history is unknown, or integer addition overflows. The sum of available totals remains explicitly partial where applicable.
- With a total-token guard enabled, unavailable complete usage pauses further work. Raising the threshold cannot resolve missing usage. A request-count-only guard can still operate when counts are known.
- With either threshold reached, pending tools are saved and remain unexecuted. A final text response that needs no further tool can complete its turn; later input or continuation checks the same cumulative limits.

Limits are checked at request/tool boundaries. A request already sent can exceed the token threshold, and cancellation cannot guarantee that the provider stopped processing it. There is no token preflight estimate, automatic retry, cost conversion, context-window estimate or percentage. Billing enforcement belongs to the provider. Probes, direct PM commands and other sessions have separate accounting. MCP traffic is not counted as model requests.

## Resume and recovery

The runtime records `LimitReached` with the reason, preserves pending tools and returns CLI exit code 2. Inspect the report and saved work first. To continue with a larger allowance, edit the explicit limits in operator configuration and then use `/continue`, or `chat --session ID` in the CLI. Configuration is loaded for the next invocation; changing it while a run is active does not change that run's guard. Increasing limits never approves an external tool. Existing clarification, approval, reconciliation and changed-Skill checks remain applicable.

Limits do not reset on input, approval, cancellation, restart or continuation. When limit-paused tools remain pending, continue them explicitly before sending a new message so protocol history stays ordered. A new session has separate accounting and does not carry pending work forward. There is no automatic reset or rollover. The normal execution record includes the configured limits and an observation summary; per-request timestamps, states and usage remain in the local runtime database.

Older sessions without a ledger remain readable, but historical counts and totals are unknown. Enabling a guard that needs those values pauses the session; the application does not reconstruct them from conversation length or the last response. With guards disabled, an explicitly resumed legacy session can continue collecting new observations while preserving the unknown earlier history.

An attempt left in `started` state after interruption retains its count and unknown usage. Failed or cancelled requests are not silently removed. The existing tool ledger separately requires reconciliation of unknown tool outcomes.

See [provider probes](Provider-Probes.md), [application setup](README.md), [interaction controls](Interaction.md) and [implementation roadmap](Implementation.md).
