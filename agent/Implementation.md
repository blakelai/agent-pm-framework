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
| Planning | Native Rust estimates/capacity/dependencies, localized Markdown reports and structured forecast data; no interpreter required |
| Other built-in tools | Framework Python behavior reused through a compiled-in toolkit and shell-free subprocess adapter |
| MCP | Official Rust SDK, stdio/Streamable HTTP, configured tool allowlist, schema validation, exact-call authorization |
| Store | SQLite sessions, tool ledger and document projection; unknown outcomes require reconciliation |
| Distribution | Cargo lockfile, local configuration example, operating guide and macOS/Linux CI jobs |

The code is an executable workbench with native planning. Existing workflow procedures cover intake through operations, but the complete product roadmap is not finished. Native traceability and governance, rich task/resource editors, an interactive schedule and visual artifact review are still pending.

## Delivery sequence and acceptance

| Milestone | Current state | Exit criteria |
|---|---|---|
| 0. Contracts and technical integration | Protocol fixture implementation complete; live provider certification pending | Both adapters and both MCP transports exercise actual request/result cycles; real endpoints validated separately |
| 1. Local workbench | Implemented | Browse real Vault records without a model; persist/restore conversations; preserve source/baseline boundaries |
| 2. Grounded analysis MVP | Initial vertical flows implemented and exercised with one live model | Sources -> explicit questions -> sufficient answers -> sourced draft -> deterministic validation; broader real-project evaluation pending |
| 3. Full delivery controls | Native Rust planning implemented; other helper procedures accessible; richer UI pending | Baseline/change gates, resource calendars, remaining forecasts, risks, UAT/release and benefits operated through typed commands and views |
| 4. Materials and reliability | Original helper ingestion accessible; visual review/OCR and advanced conflict handling pending | PDF/slides/images reviewed with locators; missing extraction never presented as complete; backup/restore and migrations exercised |
| 5. Pilot and release | Pending | Real internal-system project, live provider matrix, semantic/clarification evaluations, platform packaging and operational documentation |

## Next engineering work

1. Expand the live provider matrix beyond the tested CLIProxyAPI route, using user-specified endpoints/models. Do not infer installed model names or inspect unrelated secrets.
2. Exercise a real repository/OpenWiki snapshot and user-supplied source through BRD/PRD/REQ/PBI. Evaluate actual citations and question quality, not only tool schemas.
3. Add native Rust traceability and governance against the Python fixtures. Preserve immutable baseline pins, current requirement versions, actual execution evidence and explicit approval events. Planning parity is now covered separately.
4. Add typed editing for staffing, iterations, risks and decisions, with workflow-specific state transitions and reviewable changes.
5. Add dependency-scoped clarification gates, automatic file watching, source impact views, document diffs and richer evidence navigation.
6. Add explicit provider capability probes, budget/usage reporting, tested retry policies for proven idempotent reads, MCP OAuth, and reviewed visual extraction.

## Validation boundary

On 2026-10-15, local checks passed 25 Rust tests, 73 framework tests, formatting, Clippy and the 58-document OKF authoring profile. The Rust suite includes a differential test covering 41 planning cases against the existing Python implementation: arithmetic, unknown estimates, remaining work, calendars, cross-project FTE, dependencies, external waits, blocked inputs and localized reports. Test dispatch deliberately supplies a nonexistent Python executable to prove planning is native. Additional checks cover rejected translations, preserving prior reports and rejecting linked planning inputs. A real terminal session exercised document browsing, search, view switching and clean exit. Protocol tests used disposable loopback services and both MCP transports. GitHub CI passed on macOS and Linux for the workbench implementation and its first live-test refinements.

The user-selected CLIProxyAPI gateway and `gemini-3.8-flash-high` passed two live synthetic scenarios in a separate disposable workspace:

- Planning: document tools inspected missing inputs, `ask_clarification` persisted and surfaced a question, an explicit fixture answer supplied capacity and estimates, and `resolve_clarification` recorded the resolution. The PM tool produced a 3.00 person-day forecast with Backend/QA capacities of 7.20/3.60 person-days. Assertions checked the completed session and report.
- Draft chain: the model ingested the supplied fixture text, captured evidence, and created glossary, BRD, PRD, REQ and PBI drafts with two acceptance criteria. Assertions checked source hashes, upstream links, requirement satisfaction, draft/proposed states and unspecified effort/dates. Validation found no errors in the new chain and correctly reported a missing context in an unrelated planning fixture. Source/evidence review and unapproved-draft warnings remain expected; coverage also reported missing test execution and release evidence. No project approval was claimed.

These scenarios validate only the selected route and supplied inputs. They do not certify other providers or general model reasoning. Live runs led to fixes for redundant local-tool authorization, reading the tool manual, bounded multi-step continuation and requirement views that follow document types even outside conventional subfolders. Templates remain available in All but are excluded from project-specific views. The synthetic records and local credentials are excluded from the framework distribution.

On 2026-10-15, a further live invocation used the native Rust planner. The saved tool ledger confirmed `engine: rust`, successful completion, 3.00 scheduled person-days, one unestimated item and capacities of 7.20/3.60 person-days. The model reported those actual results without changing fixture inputs.

Automated tests use synthetic temporary data and local protocol servers. They exercise model protocol handling and runtime behavior, not the quality of a real model's reasoning. A local `doctor` result is not a provider certification. Structural OKF checks do not prove language quality or truth of claims. The current UI renders Markdown text and properties; diagrams and original media open through desktop tools.

See [application usage](README.md), [framework workflow](../00_Governance/Workflow.md) and [evaluation method](../90_Agent/Evals/Evaluation-Method.md).
