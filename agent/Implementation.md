---
type: "Reference"
title: "PM Agent implementation milestones"
language: "en"
---

# PM Agent implementation milestones

The approved direction is a local Rust TUI for macOS/Linux, sharing project documents through Git. Required provider profiles are OpenAI, Ollama, vLLM, CLIProxyAPI and OpenRouter. No development staffing, availability or target dates were supplied; this plan does not invent them.

## Working implementation

| Module | Implemented behavior |
|---|---|
| Workspace | Obsidian-compatible document scan/read/search, source hashes, draft state and language checks, stale-write rejection, path/symlink restrictions |
| TUI | Eleven document views, content and properties, streaming conversation, command input, local PM commands, resume/approval/reconciliation controls |
| Provider | Responses and Chat Completions SSE adapters, fragmented UTF-8 and function arguments, local conversation replay, error/cancellation handling |
| Agent runtime | Selected existing Skill/Playbook, tool loop, persisted questions, user answers with provenance, explicit resolution, sourced draft writes |
| Built-in tools | Framework Python behavior reused through a compiled-in toolkit and shell-free subprocess adapter |
| MCP | Official Rust SDK, stdio/Streamable HTTP, configured tool allowlist, schema validation, exact-call authorization |
| Store | SQLite sessions, tool ledger and document projection; unknown outcomes require reconciliation |
| Distribution | Cargo lockfile, local configuration example, operating guide and macOS/Linux CI jobs |

The code is an initial executable increment. Existing workflow procedures cover intake through operations, but the complete product roadmap is not finished. Native Rust PM calculations, rich task/resource editors, an interactive schedule and visual artifact review are still pending.

## Delivery sequence and acceptance

| Milestone | Current state | Exit criteria |
|---|---|---|
| 0. Contracts and technical integration | Protocol fixture implementation complete; live provider certification pending | Both adapters and both MCP transports exercise actual request/result cycles; real endpoints validated separately |
| 1. Local workbench | Implemented | Browse real Vault records without a model; persist/restore conversations; preserve source/baseline boundaries |
| 2. Grounded analysis MVP | Initial vertical flow implemented | Sources -> explicit questions -> sufficient answers -> sourced draft -> deterministic validation; real-model behavior evaluation still required |
| 3. Full delivery controls | Existing helper procedures accessible; native Rust port and richer UI pending | Baseline/change gates, resource calendars, remaining forecasts, risks, UAT/release and benefits operated through typed commands and views |
| 4. Materials and reliability | Original helper ingestion accessible; visual review/OCR and advanced conflict handling pending | PDF/slides/images reviewed with locators; missing extraction never presented as complete; backup/restore and migrations exercised |
| 5. Pilot and release | Pending | Real internal-system project, live provider matrix, semantic/clarification evaluations, platform packaging and operational documentation |

## Next engineering work

1. Use a user-specified endpoint/model for the first live provider acceptance run. Do not infer installed model names or inspect unrelated secrets.
2. Exercise a real repository/OpenWiki snapshot and user-supplied source through BRD/PRD/REQ/PBI. Evaluate actual citations and question quality, not only tool schemas.
3. Add native Rust planning and traceability modules against the Python fixtures. Retain exact handling of unknown values, remaining effort, cross-project allocations and immutable baseline pins.
4. Add typed editing for staffing, iterations, risks and decisions, with workflow-specific state transitions and reviewable changes.
5. Add dependency-scoped clarification gates, automatic file watching, source impact views, document diffs and richer evidence navigation.
6. Add explicit provider capability probes, budget/usage reporting, tested retry policies for proven idempotent reads, MCP OAuth, and reviewed visual extraction.

## Validation boundary

On 2026-10-14, local checks passed 22 Rust tests, 73 framework tests, formatting, Clippy and the 58-document OKF authoring profile. A real terminal session exercised document browsing, search, view switching and clean exit. Protocol tests used disposable loopback services and both MCP transports. The user-supplied local gateway responded to a model-list request with HTTP 401 (missing API key); live model validation requires the key environment-variable name and model ID. No live model output has been accepted as evidence.

Automated tests use synthetic temporary data and local protocol servers. They exercise model protocol handling and runtime behavior, not the quality of a real model's reasoning. A local `doctor` result is not a provider certification. Structural OKF checks do not prove language quality or truth of claims. The current UI renders Markdown text and properties; diagrams and original media open through desktop tools.

See [application usage](README.md), [framework workflow](../00_Governance/Workflow.md) and [evaluation method](../90_Agent/Evals/Evaluation-Method.md).
