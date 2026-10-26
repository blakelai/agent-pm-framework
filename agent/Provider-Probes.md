---
type: "Reference"
title: "Explicit provider capability probes"
language: "en"
---

# Explicit provider capability probes

Run an isolated check against a configured provider before relying on its text and function-call behavior. The check records observations from the actual selected endpoint and adapter. A profile name, configured model label or successful local configuration check does not establish provider capability.

## Run or inspect

| Interface | Explicit network check | Read latest local result |
|---|---|---|
| CLI | `pm-agent --vault PATH probe` | `pm-agent --vault PATH probe --last` |
| CLI, selected profile | `pm-agent --vault PATH probe --profile NAME` | `pm-agent --vault PATH probe --profile NAME --last` |
| TUI Chat | `/probe` or `/probe NAME` | `/probe last` or `/probe last NAME` |

The selected profile must contain an explicit endpoint and model. Profile selection for a probe does not change the active profile or an existing Agent session. Names with spaces should use the CLI's quoted profile argument.

Only the explicit network command starts requests. It sends at most three synthetic requests; service charges may apply. With `tools = false`, only the text check runs. Each request uses the configured timeout capped at 60 seconds. This is a request/time bound, not a token or monetary budget. There are no automatic retries, protocol fallbacks, model substitutions or configuration changes.

In Chat, Esc in the composer requests cancellation; Esc in the transcript first returns to the latest output and composer. Ctrl+C clears the input draft. The CLI uses Ctrl+C to cancel. Cancellation preserves partial observations and sends no follow-up request; the service may still process a request already sent.

## Observed checks

1. **Text stream:** a completed SSE response must deliver a text delta and the exact synthetic marker. Empty, truncated or unexpected output does not pass.
2. **Function call:** one returned call must match the local synthetic function and its exact nonce schema. Unknown, additional or mismatched calls fail the check; they are never executed.
3. **Tool-result continuation:** the application supplies a new receipt only in the synthetic tool result. The next response must return that receipt without further calls. This tests the selected adapter's continuation path, including preserved Responses reasoning items.

Checks report Passed, Failed, Skipped or Cancelled. A failure means the requested behavior was not observed in that run; it does not establish permanent lack of support. Authentication failures, HTTP errors and malformed streams stop dependent checks. Disabled tools are marked untested rather than unsupported. CLI live checks return exit code 2 when a required observation fails or is cancelled. Reading a valid saved report succeeds even when its observations failed; inspect the check states and configuration match.

Input, output and total token values are retained separately for each request, only when supplied by the service. Zero remains zero; missing fields remain unavailable. The probe does not infer totals, context windows, remaining capacity, cost or percentages. Probe usage does not replace the active conversation's usage.

## Isolation and freshness

The probe calls the existing provider adapter directly. It does not read project documents, load Skills, replay conversations, initialize MCP servers or expose PM tools. Prompts contain only fixed test instructions and random markers. Raw response text, function arguments, reasoning payloads, HTTP error bodies and credential values are not retained in the report.

The latest observation is stored under ignored `.pm-agent/probes/<profile-name-hash>.json`. A new run replaces the previous observation for that profile, including when it fails or is cancelled. This is private runtime JSON, not a project knowledge document, approval or shared baseline. No report is copied into the distributed framework automatically.

Each record identifies the configured model, protocol, timestamps, request attempts, elapsed time and a fingerprint of the selected configuration. Endpoint, model, flags, timeout or key-environment-variable name changes mark the old result as historical. Missing current configuration makes freshness unavailable. Credential value changes and remote deployment changes cannot be detected by this fingerprint. A gateway's actual routed model is not independently verified.

`probe --last` and `/probe last` make no model request and need no key value. Missing, corrupted, oversized or linked records are rejected rather than replaced with an older success. `doctor` remains a local configuration check, and starting the TUI never runs probes automatically.

## Protocol references and limits

The flow reuses the existing Responses and Chat Completions adapters. OpenAI documents the request, function-call, tool-output and continuation sequence in [Function calling](https://developers.openai.com/api/docs/guides/function-calling), and SSE response handling in [Streaming API responses](https://developers.openai.com/api/docs/guides/streaming-responses), consulted on 2026-10-26. These references describe OpenAI behavior; compatibility of another service is established only by actual observations.

Protocol fixtures cover both adapters. Real-provider observations remain bounded to the selected route, model label, settings and test time. This feature does not certify semantic reasoning, project analysis quality, vision/OCR, MCP authentication, reliability under load or provider-wide support.

See [application setup](README.md), [interaction controls](Interaction.md) and [implementation roadmap](Implementation.md).
