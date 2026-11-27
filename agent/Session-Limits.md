---
type: "Reference"
title: "Session usage and optional reported-token guard"
language: "en"
---

# Session usage and optional reported-token guard

Normal Chat and CLI execution have no request-count or step-count cap. Work continues until completion, a clarification or approval boundary, an execution error, or manual cancellation. In TUI Chat, Esc in the composer cancels the active execution; Ctrl+C clears the input. Esc in the conversation pane scrolls to the latest content and returns focus to the composer. CLI Chat uses Ctrl+C for cancellation.

Legacy `max_steps` and `session_limits.max_requests` configuration fields remain readable but are ignored in interactive execution and local budget reports. They do not need to be raised or removed to continue. Requests remain counted as observations. Evaluation-only caps are supplied explicitly by [the evaluator](Evaluation.md), separately for each test stage.

## Configure and inspect reported usage

The optional token guard is disabled by default. To enable it, add this top-level table outside provider and MCP tables; the value is an example, not a recommended budget:

```toml
[session_limits]
max_total_tokens = 50000
```

Zero pauses immediately; negative values and unknown keys are rejected. `/budget` in Chat, `pm-agent --vault PATH budget`, and `budget --session ID` read local configuration and observations without calling a model, starting MCP or changing session state. The composer draft is preserved. Context displays only the latest service-reported fields; cumulative observations are separate.

Each request attempt is saved before dispatch, including failed, cancelled and interrupted attempts. Counts do not establish delivery or billing. Exact service-reported input/output/total fields are retained, including zero. A complete session total is unavailable when earlier history or any total is missing, or summation overflows. Totals are never inferred from input plus output or conversation length.

If an explicitly enabled token guard cannot determine a complete total, or its threshold is reached, it records LimitReached before the next request or pending tool. A final response may complete without another request. An in-flight request can exceed the threshold; this is not a cost cap. There is no automatic retry, context-window estimate or cost conversion. Provider timeouts, local context-size bounds and tool integrity checks remain effective.

## Cancellation, continuation and recovery

Cancellation preserves the session and known tool outcomes. A provider may continue an already dispatched request. Cancellation during a tool may leave an uncertain outcome requiring reconciliation; continuation never blindly repeats it. Clarification, exact external-call approval and changed-Skill checks remain effective.

For a token-guard pause, inspect recorded work and usage, explicitly change or disable that guard if appropriate, then use `/continue` or CLI `chat --session ID`. Configurations are read on the next invocation. Continuation does not reset accounting or approve pending calls. A session paused by a retired request cap can continue with the same history and pending work; no request allowance must be replenished.

Older sessions remain readable. Missing historical counts and usage stay unknown, and new observations are recorded without reconstructing the past. Probes, direct PM operations and other sessions have separate accounting. See [interaction controls](Interaction.md) and [application setup](README.md).
