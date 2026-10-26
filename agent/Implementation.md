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
| TUI | Separate Browse/Chat modes, eleven document categories, shared Markdown rendering, mouse/scrollbars, collapsible/maximized reading, multiline input and focus-aware cancellation |
| File refresh | Background content polling for knowledge documents, baselines and catalogs; automatic browse/planning/delivery updates, retained drafts and stale-preview invalidation |
| Structured editing | Human-operated forms for allocations, iterations, forecast dates, PBIs, risks and draft decisions; reviewed diffs, existing readiness/planning checks, optimistic conflict checks and edit records |
| Provider probes | Explicit isolated CLI/Chat checks for completed SSE text, schema-matched function calls and tool-result continuation; private observations, configuration freshness and service-only usage |
| Session limits | Optional cumulative request and reported-token guards; durable pre-dispatch attempts, explicit limit pauses, unknown legacy/missing totals and local CLI/Chat inspection |
| Provider | Responses and Chat Completions SSE adapters, fragmented UTF-8 and function arguments, local conversation replay, error/cancellation handling |
| Clarification | Explicit project/record scopes, transitive draft/baseline gates across sessions, independent continuation, answer provenance and read-only Questions view |
| Agent runtime | Stage Skill/Playbook plus explicit custom Skills, dependency/reference validation and activation fingerprints; tool loop, persisted questions and sourced draft writes |
| Planning | Native Rust estimates/capacity/dependencies, localized Markdown reports and structured forecast data; no interpreter required |
| Interactive planning | Iteration-filtered PBIs, role capacity/demand, supplied allocations and project-wide signals; reviewed single-item scheduling with before/after planning impact |
| Delivery workflows | Dedicated Change/UAT/Release/Benefit views and typed editors; versioned evidence gates, historical outcome preservation, read-only Agent overview |
| Traceability and governance commands | Native coverage/status/impact, baseline capture and pinned fingerprints, settings and work packets; compatible legacy content hashes |
| Validation | Native language, terminology, traceability, readiness/acceptance, source and baseline integrity, local OKF authoring checks; structured findings and strict-mode exit status |
| Source operations | Native material import and PDF/Office text extraction, repository registration, immutable snapshots and hash-verified evidence capture |
| Material inspection | Local source catalog, explicit bounded original/extraction checks, claim locators and review gaps; background TUI inspection and rechecked desktop opening |
| Material review records | Human-operated version-bound review forms, actual scope/actor/time/method/outcome, explicit correction history and shared current-review checks; originals and claims remain unchanged |
| Impact and comparison | Read-only reference chains, explicit baseline/current and pinned/latest source comparisons; background TUI reads, integrity checks and stale-result invalidation |
| OKF exchange | Native bundle validation and staged portable export; exact raw attachments and typed Skill concepts |
| MCP | Official Rust SDK, stdio/Streamable HTTP, configured tool allowlist, schema validation, exact-call authorization |
| Store | SQLite sessions, tool ledger and document projection; unknown outcomes require reconciliation |
| Distribution | Cargo lockfile, local configuration example, operating guide and macOS/Linux CI jobs |

The executable workbench now has 19 native PM commands. Production execution and packaging no longer use Python, an embedded toolkit or an interpreter setting. Existing workflow procedures cover intake through operations. Structured editors, interactive scheduling/resource views, initial dedicated delivery workflows and human material review records are implemented; automated visual extraction, advanced planning and broader live-project evaluation remain on the product roadmap.

## Delivery sequence and acceptance

| Milestone | Current state | Exit criteria |
|---|---|---|
| 0. Contracts and technical integration | Protocol fixture implementation complete; live provider certification pending | Both adapters and both MCP transports exercise actual request/result cycles; real endpoints validated separately |
| 1. Local workbench | Implemented | Browse real Vault records without a model; persist/restore conversations; preserve source/baseline boundaries |
| 2. Grounded analysis MVP | Initial vertical flows implemented and exercised with one live model | Sources -> explicit questions -> sufficient answers -> sourced draft -> deterministic validation; broader real-project evaluation pending |
| 3. Full delivery controls | Planning and delivery views implemented; advanced planning and broader release scopes pending | Baseline/change gates, resource calendars, remaining forecasts, risks, UAT/release and benefits operated through typed commands and views |
| 4. Materials and reliability | Native ingestion, inspection and human review records implemented; automated vision/OCR, semantic evaluation and advanced conflict handling pending | PDF/slides/images reviewed with locators; missing extraction never presented as complete; backup/restore and migrations exercised |
| 5. Pilot and release | Pending | Real internal-system project, live provider matrix, semantic/clarification evaluations, platform packaging and operational documentation |

## Next engineering work

1. Extend [delivery workflows](Delivery-Workflows.md) after real-project evaluation: multi-baseline or non-requirement release contracts, cost aggregation, explicit history migration and richer evidence navigation. Existing forms and scheduling are described in [Editing](Editing.md) and [Planning view](Planning-View.md). Bulk schedule scenarios and a calendar-duration model need separate requirements; person-days alone cannot establish task dates.
2. Evaluate [scoped clarification](Clarification.md) with real project questions and extend [impact and comparison](Inspection.md) after usage feedback: material revision contracts, broader relationship types and richer evidence navigation. [Automatic refresh](Automatic-Refresh.md) now uses background content polling; operating-system notifications and automatic conflict reconciliation remain possible reliability improvements.
3. Extend [provider probes](Provider-Probes.md) and [session limits](Session-Limits.md) with provider-reported capacity and explicitly sourced monetary budgets, tested retry policies for proven idempotent reads, MCP OAuth, and reviewed visual extraction. The [Materials workbench](Materials.md) provides source/extraction/evidence inspection and [human review records](Material-Reviews.md); OCR, authenticated review attestation and semantic evaluation remain separate work. Current session guards use request attempts and reported total tokens only; they do not infer context windows or costs.
4. Expand the live provider matrix using user-specified endpoints/models and evaluate a real repository/OpenWiki/source workflow. Installed model names, business sources and semantic decisions must not be inferred.

## Validation boundary

On 2026-10-15, local checks passed 29 Rust tests, 73 framework tests, formatting, Clippy and the 58-document OKF authoring profile. The Rust suite includes a differential test covering 41 planning cases against the existing Python implementation: arithmetic, unknown estimates, remaining work, calendars, cross-project FTE, dependencies, external waits, blocked inputs and localized reports. Planning dispatch is native; the current binary acceptance test runs with an empty executable search path. Additional checks cover rejected translations, preserving prior reports and rejecting linked planning inputs. A real terminal session exercised document browsing, search, view switching and clean exit. Protocol tests used disposable loopback services and both MCP transports. GitHub CI passed on macOS and Linux for the workbench implementation and its first live-test refinements.

Native governance adds 34 differential scenarios covering coverage, status, downstream impact, candidate/approved/rescinded baselines, current and historical fingerprints, simulations and conflicting test results. Semantic-hash parity covers 262 numeric/Unicode cases and six legacy scalar/list representations. Rust-created baselines are readable by the original Python implementation. Settings/work-packet tests confirm that language changes do not translate existing records or create approvals. Duplicate IDs, linked active records and escaping snapshot paths are rejected.

On 2026-10-16, native validation increased the Rust suite to 33 passing tests. Its 79 differential scenarios compare finding severity, code, path, counts and default/strict exit behavior with the original validator; selected human-facing messages are also compared exactly. Cases cover language settings, terminology, missing or stale source evidence, actual readiness records, approved and rescinded baselines, pinned-version acceptance, excluded unrelated test passes and the local OKF profile. Additional tests check malformed/non-UTF-8 records, symlinks, invalid settings through the actual CLI, and exclusion of private/raw material and native Skill adapters. Invalid records remain visible as findings while independent checks continue. Frontmatter ending at EOF is accepted consistently with existing notes. These checks do not prove semantic completeness or authorize project approvals.

The source/OKF migration completes the 15 native PM operations and adds seven Rust tests. They compare nine material cases and nine bundle-validation cases with the independent oracle, including exact snapshot IDs, evidence metadata, export maps and exported concept bodies. Further checks cover blank/encrypted/malformed PDFs, Office size limits and external entities, original-byte preservation, snapshot tampering, exclusion rules, path escapes, maximum-length repository IDs and Git state reads without source-repository hooks. An actual binary test runs PM command dispatch and PDF ingestion with an empty PATH. The original Python implementation remains exclusively under tests/legacy, with development-only dependencies under tests/requirements.txt.

The user-selected CLIProxyAPI gateway and `gemini-3.8-flash-high` passed two live synthetic scenarios in a separate disposable workspace:

- Planning: document tools inspected missing inputs, `ask_clarification` persisted and surfaced a question, an explicit fixture answer supplied capacity and estimates, and `resolve_clarification` recorded the resolution. The PM tool produced a 3.00 person-day forecast with Backend/QA capacities of 7.20/3.60 person-days. Assertions checked the completed session and report.
- Draft chain: the model ingested the supplied fixture text, captured evidence, and created glossary, BRD, PRD, REQ and PBI drafts with two acceptance criteria. Assertions checked source hashes, upstream links, requirement satisfaction, draft/proposed states and unspecified effort/dates. Validation found no errors in the new chain and correctly reported a missing context in an unrelated planning fixture. Source/evidence review and unapproved-draft warnings remain expected; coverage also reported missing test execution and release evidence. No project approval was claimed.

These scenarios validate only the selected route and supplied inputs. They do not certify other providers or general model reasoning. Live runs led to fixes for redundant local-tool authorization, reading the tool manual, bounded multi-step continuation and requirement views that follow document types even outside conventional subfolders. Templates remain available in All but are excluded from project-specific views. The synthetic records and local credentials are excluded from the framework distribution.

On 2026-10-15, a further live invocation used the native Rust planner. The saved tool ledger confirmed `engine: rust`, successful completion, 3.00 scheduled person-days, one unestimated item and capacities of 7.20/3.60 person-days. The model reported those actual results without changing fixture inputs.

Automated tests use synthetic temporary data and local protocol servers. They exercise model protocol handling and runtime behavior, not the quality of a real model's reasoning. A local `doctor` result is not a provider certification. Structural OKF checks do not prove language quality or truth of claims. The UI renders terminal Markdown and exposes properties separately; diagrams and original media open through desktop tools.

See [application usage](README.md), [framework workflow](../00_Governance/Workflow.md) and [evaluation method](../90_Agent/Evals/Evaluation-Method.md).

## TUI and custom Skill iteration

The agreed interface and loading behavior are documented in [Interaction](Interaction.md) and [Custom Skills](Skills.md). Both protocol adapters retain service-reported usage; absent values are not estimated. Skills are explicitly selected, validated and recorded with content hashes. Changed Skill instructions cannot silently govern a pending tool operation. Automated event, rendering and local-provider tests accompany this iteration; real terminal and provider coverage remain bounded by the recorded checks.
