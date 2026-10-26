---
type: "Reference"
title: "PM Agent application"
language: "en"
---

# PM Agent application

A working first implementation of the local Rust terminal agent. It browses the existing Obsidian workspace, runs its project-management tools, and uses a configured model to read evidence, ask questions and write sourced drafts. macOS and Linux are the target platforms. See [implementation status](Implementation.md) for completed work, tests and remaining milestones.

## Build and start

Run from the framework repository root. Rust 1.96+ and a C toolchain are required. The application and SQLite are compiled into the binary. All 18 PM operations run natively in Rust, including PDF/Office text extraction and OKF export. No Python installation is required to build or run the application. Git records repository revision/dirty state when available; unavailable Git produces an explicit unversioned snapshot.

```sh
cargo build --manifest-path agent/Cargo.toml --locked
agent/target/debug/pm-agent --vault . tui
```

Browsing, searching and opening documents do not need an API key or model. F1 shows help; F2 switches directly to chat input. Edit project content in Obsidian or use `o` to open the selected document with the desktop's associated application. Use `r` to reload changes.

Use a dedicated project copy for actual work. The framework checkout contains no demonstration requirements, staffing or dates. Keep [Project](../00_Governance/Project.md) and [Settings](../00_Governance/Settings.md) authoritative; a conversation's language does not change the document language.

## Configure a model

```sh
mkdir -p .pm-agent
cp agent/config.example.toml .pm-agent/config.toml
```

Edit `.pm-agent/config.toml` locally. Select the provider profile, its actual model ID and endpoint. When upgrading an older configuration, remove its obsolete top-level `python` field; `pm --python` has also been retired. Model IDs are deliberately empty in the distributed configuration. Keys are read from named environment variables; do not put key values into the config, project documents or chat. Start the application from a shell that has those variables set.

| Profile | Default protocol | Endpoint convention | Verification status |
|---|---|---|---|
| OpenAI | Responses | `https://api.openai.com/v1` | Protocol fixture tested; live account/model pending |
| Ollama | Chat Completions | `http://127.0.0.1:11434/v1` | Protocol fixture tested; installed model pending |
| vLLM | Chat Completions | `http://127.0.0.1:8000/v1` | Protocol fixture tested; model/parser configuration pending |
| CLIProxyAPI | Chat Completions | configurable local gateway | Live `gemini-3.8-flash-high` route: streaming, tools, clarification/resume, draft chain and synthetic forecast tested; explicit text/call/result probe and service-reported usage observed on 2026-10-26 |
| OpenRouter | Chat Completions | `https://openrouter.ai/api/v1` | Protocol fixture tested; selected route/model pending |

The example local ports are editable configuration examples, not discovered services. Both adapters stream output and preserve function-call/result context. Responses uses local history and requests encrypted reasoning for stateless continuation. Compatible implementations that reject an optional Responses field should use their tested Chat Completions profile; there is no automatic protocol fallback. `tools = true` enables function declarations but does not certify a model's tool capability. Set it to false for text-only assistance.

```sh
agent/target/debug/pm-agent --vault . doctor
agent/target/debug/pm-agent --vault . chat --stage prd "Review the supplied sources and ask about missing requirements."
```

[Explicit provider probes](Provider-Probes.md) are available through `probe` / `probe --last` in the CLI and `/probe` / `/probe last` in Chat. They test completed SSE text, one synthetic function call and a tool-result receipt, with service-reported usage. An explicit run sends at most three synthetic requests and may incur service charges; saved observations are private and bound to the selected configuration.

[Session limits](Session-Limits.md) optionally cap cumulative request attempts and service-reported total tokens across resumes. `/budget` or CLI `budget --session ID` shows local observations. Limits are unset by default; missing totals remain unknown, and a token guard pauses when a complete total is unavailable. An in-flight request can exceed the threshold; this is not a hard cost cap.

`doctor` checks local configuration and key presence. It does not send a live model request. Endpoint authentication, route availability, model quality, tool support and usage accounting still require live acceptance tests. HTTPS is required for remote endpoints; HTTP is allowed on loopback.

## Use the terminal workbench

| Control | Action |
|---|---|
| F2 / click mode label | Switch Browse and Agent Chat, preserving drafts and reading state |
| Alt+1–9 / Alt+0 | Select the first nine categories / open all eleven categories |
| Alt+Left / Alt+Right | Previous / next category |
| Enter in document list / Esc in document | Focus document / return to the list |
| F3 / F4 / F6 | Collapse list / maximize document / toggle emoji |
| Wheel / scrollbar drag | Scroll the pointed pane or its rendered content |
| Right-click / L / P | Context actions / document links / properties |
| O / R in Browse | Open original / refresh documents |
| E / N in Browse | Edit supported project fields / create a PBI, risk or decision |
| F5 / Planning in Browse | Interactive iteration, capacity and planning-signal views |
| F7 / Delivery in Browse | Change, UAT, release and benefit records, evidence checks and editors |
| F8 / Impact and compare in context menu | Recorded downstream chains and explicit baseline/source differences |
| F9 / Questions in context menu | Question scopes, affected records, response provenance and resolution checks |
| Enter in Chat | Send the draft when idle; input is ready without pressing i |
| Alt+Enter / supported Shift+Enter / Ctrl+J | Insert a newline; multiline paste never sends |
| Esc in transcript | Latest output, resume following, return input focus |
| Esc in composer | Request cancellation when running; retain the draft |
| Ctrl+C in Chat | Clear only the input; do not cancel or exit |
| Ctrl+Q / q in Browse / /quit in Chat | Exit when idle |
| F1 | Show help |

The transcript and composer are borderless; the input has a gray background and supports multiple lines, cursor movement and Unicode grapheme deletion. The browser and conversation render Markdown. The status line shows the model, state, short session ID and service-reported input usage; click it or use `/status` for full details. Missing usage remains unavailable, with no estimates or inferred percentages.

See the [interaction contract](Interaction.md) for focus priority, terminal compatibility, link handling and cancellation semantics.

F8 or the document context menu opens [Impact and Compare](Inspection.md). Follow recorded reference chains, explicitly select a baseline to compare, or compare repository evidence against the latest synchronized snapshot. Changes clear stale results; R revalidates. Missing inputs remain visible and no business state is inferred from text differences.

[Automatic refresh](Automatic-Refresh.md) detects local knowledge edits every two seconds without a model call. Browse, Planning and Delivery update while retaining selection and drafts. Changed inputs invalidate an open form's preview; unreadable files show an error and retry automatically. Historical reports are not regenerated.

[Structured editing](Editing.md) covers staffing, iterations, forecast dates, PBIs, risks and draft decisions. Ctrl+S first validates and previews changes; a separate Ctrl+S saves the reviewed result. Missing values remain unknown. Status changes require the existing evidence contracts, and stale documents or changed preview inputs are rejected.

Views include BRD, PRD/REQ, domain knowledge, systems, questions, people/schedules, backlog, progress/risks, verification and Agent runs. They show real Markdown records and metadata. Requirement views recognize document types even outside conventional subfolders. Templates appear in All, not project-specific views. Empty views remain empty; no progress, people or risk scores are synthesized for presentation.

The [planning view](Planning-View.md) filters actual PBIs by iteration, shows capacity and remaining demand by role, and exposes supplied allocations across projects. M proposes a PBI move; review its before/after planning impact before saving. P, I and A open the existing allocation, iteration and forecast-date editors. Missing inputs keep calculations unavailable, and refreshing the view never replaces a historical forecast report.

The [delivery views](Delivery-Workflows.md) show current workflow checks and missing inputs. N creates a draft in the selected workflow; E edits it through the same review/save controls. UAT uses approved requirement versions and actual tests, release requires matching UAT and operational evidence, and benefit measurements preserve supplied baselines and observation windows. Recorded outcomes remain historical; these screens do not approve or deploy automatically.

| Command | Purpose |
|---|---|
| `/skills`, `/skills clear` | Select project Skills / clear additional selections |
| `/probe [PROFILE]`, `/probe last [PROFILE]` | Explicit synthetic provider check / local observation replay; leaves the current Agent session unchanged |
| `/status` | Full session, active selection and service-reported usage details |
| `/budget` | Current operator limits and cumulative session observations; local query without a model request |
| `/search text` | Literal search, including Chinese and code identifiers; `/search` clears it |
| `/stage prd` | Select a workflow capability before starting a session |
| `/pm plan` | Recalculate a staffing forecast from recorded inputs |
| `/pm validate`, `/pm coverage`, `/pm status` | Generate and display actual PM reports |
| `/pm delivery` | Read current delivery records and missing gate inputs without writing a report |
| `/pm inspect ID` | Read downstream reference chains; add `--baseline BASE-ID` or `--source` for an explicit comparison |
| `/pm questions` | Read recorded question scopes and current blockers without a report or model |
| `/answer QUESTION-ID TEXT` | Associate actual input with one open question; resume its originating session first |
| `/continue-independent` | Resume supported independent work; pending questions stay open |
| `/sessions`, `/resume ID` | List or load durable conversations |
| `/continue` | Explicitly continue a failed, cancelled, completed or limit-paused turn; current limits are checked again |
| `/approve`, `/deny` | Decide the exact pending external/import operation |
| `/reconcile actual outcome` | Record what happened to an interrupted operation without repeating it |
| `/new` | Start a separate session, retaining the previous one |

Stages are `workflow`, `intake`, `system`, `language`, `brd`, `prd`, `backlog`, `estimate` and `review`. The runtime loads the corresponding existing Skill and Playbook. Additional [custom project Skills](Skills.md) can be explicitly selected with a leading `$skill-name` or CLI `chat --skill NAME`; use `pm-agent --vault . skills` to inspect the catalog. Slash PM arguments currently split on whitespace; use the CLI for paths containing spaces or more complex arguments. Do not send secrets through either interface.

## Built-in project operations

Every PM operation uses the compiled Rust implementation. There is no Python dispatch, embedded Python toolkit or interpreter configuration.

| Capability | Commands |
|---|---|
| Analysis sources | `ingest`, `repo-add`, `sync`, `evidence` |
| Planning and governance | `plan`, `validate`, `coverage`, `status`, `delivery`, `questions`, `inspect`, `impact`, `baseline`, `fingerprint`, `settings`, `prepare` |
| Knowledge exchange | `okf-export`, `okf-check` |

The original implementation now lives only in `tests/legacy` as an independent regression oracle. Python is a development-test dependency, documented in [test setup](../tests/README.md); the binary does not embed or invoke those files.

`/pm plan` and CLI `pm plan` calculate and write the forecast in Rust, without Python or a model. The result includes structured capacity and item data alongside the Markdown report. It uses the existing three-point estimates, remaining availability, cross-project FTE checks, dependency rules and explicit unknown values. An invalid planning input writes a blocked report. An unavailable or invalid language catalog leaves the prior report intact and returns an error; it never silently publishes an English fallback.

Native coverage requires current requirement version/hash tokens, execution metadata and evidence. Simulations never qualify; a later failed result supersedes an earlier pass, and a simultaneous nonpassing result wins conservatively. Baseline capture reserves a new ID, copies exact bytes and publishes its manifest last; interrupted incomplete candidates cannot qualify for approval. Approval remains a separate recorded decision matching the manifest hash. Native fingerprints preserve the legacy sorted-JSON hash and flat Properties interpretation, including Unicode, float precision and workflow annotation exclusions, so old baselines remain readable.

Native `validate` writes an English quality report and returns structured findings with severity, code, path and explanation. It checks language settings, links, terminology, document/delivery state, traceability, readiness records, current source hashes, snapshot integrity, approved baselines and the local OKF authoring profile. Completed feature/migration work must cite passing test runs for its pinned requirement versions; unrelated passing runs do not satisfy acceptance. Malformed records are reported while independent checks continue. Return code 1 indicates errors, or warnings with `--strict`; 2 indicates an operation could not complete. Validation never invents missing evidence, approves work or proves semantic completeness. Source files are hashed as a stream; linked files and paths outside registered source roots are rejected.

Native source ingestion preserves original bytes and generates project-language wrappers. PDF pages, slide numbers and document text remain linked to the original; encryption, malformed input and extraction failures remain visible. Snapshot synchronization reads only registered roots, preserves OpenWiki Claims files without claiming official validation, and verifies immutable snapshot hashes before reuse or evidence capture. Language catalogs remain in the project's `tools/locales` directory and are checked against the compiled English key/placeholder contract.

OKF export stages a fresh external bundle and validates it before publication. Raw Markdown attachments keep exact bytes with a `.txt` suffix; native Skills become typed knowledge concepts. Bundle checks apply core OKF rules to every Markdown file, while authoring checks apply this project's stricter profile and raw/private exclusions.

```sh
agent/target/debug/pm-agent --vault . pm validate
agent/target/debug/pm-agent --vault . pm repo-add --id service --path /absolute/repository --wiki openwiki --code-root src
agent/target/debug/pm-agent --vault . pm sync --repo service
agent/target/debug/pm-agent --vault . pm ingest /absolute/material.pdf
```

Supported operations: settings, ingestion, repository registration/snapshot, evidence capture, work packets, validation, coverage, planning, status, impact, fingerprint, immutable baseline candidates and OKF export/check. See [the tool manual](../tools/README.md) for their contracts. PDF and Office text extraction are compiled into the binary. Images and incomplete PDF/slide extraction retain the original and require reviewed descriptions; this version does not claim full visual understanding or OCR.

The Agent can create reversible reports, work packets, evidence and baseline candidates, refresh registered repositories, and ingest existing workspace assets without another authorization prompt. Adding external material/repository scopes, changing the document language, exporting outside the workspace and invoking MCP tools requires an exact-call decision. A baseline candidate is never an approval.

O/M/P effort, availability and dates must be supplied. Unknown capacity is not zero. Imported Wiki descriptions, static code observations, actual test results and requested future behavior remain distinct evidence. A source hash check establishes version identity, not semantic correctness.

## MCP tools

Configure `mcp` profiles in the local config using the commented examples. The official Rust MCP SDK handles initialization and transport negotiation for stdio and Streamable HTTP. Stdio commands run only from operator configuration. Their environment is restricted to basic process variables and explicit `env_names`; server stderr is suppressed to avoid disrupting the terminal or persisting accidental secrets.

Only tools listed in `allowed_tools` are exposed to the model. Every model-requested MCP invocation displays its exact tool and arguments for approval. Schemas are checked locally; external schema references are rejected. Tool descriptions/results remain untrusted data. Remote credentials use an environment variable. OAuth discovery, interactive OAuth login, server-requested sampling and elicitation are not implemented in this milestone.

## Clarification, recovery and persistence

`ask_clarification` creates a project-language Question, displays it and pauses the run; all later calls in that response are discarded. [Scoped clarification](Clarification.md) records a project-wide or explicit record scope. `/continue-independent` resumes local work without answering the question. Drafts and baseline candidates remain blocked for the specified records and their recorded dependents, across sessions. Actual replies are bound to the current question and scope; receipt alone does not resolve the question. Multiple open questions require `/answer QUESTION-ID TEXT`. MCP calls remain blocked while open question scopes cannot establish their independence. Legacy sessions retain their original conservative draft gate.

Draft writes require typed frontmatter, project language metadata, draft state and current source hashes. Existing documents require the expected content hash. Approved or pinned documents cannot be overwritten. Updates use a temporary file and atomic replacement. The app locks out another PM Agent writer; external editors do not honor that lock, so hash checks reduce conflicts but are not a distributed editing transaction. Avoid editing a file during its Agent write.

`.pm-agent/runtime.sqlite3` contains durable sessions, tool arguments/results and a rebuildable document projection. It is not a disposable cache. Back up the entire `.pm-agent` directory while the app is closed. The directory is ignored by Git and OKF export, as are build artifacts and local environment files. Shared English execution summaries live under `90_Agent/Runs`; raw user-message assets and generated drafts belong to the project copy.

An interrupted tool has an unknown outcome until reconciled. Restarting does not automatically repeat it. Rate limits, timeouts and provider errors are surfaced without retries. Provider profile changes require a new session. Context and step limits stop explicitly rather than silently dropping evidence. A completed Agent turn is not a project approval, semantic verification or release.

## Verification and references

```sh
python3 -m venv .venv
.venv/bin/python -m pip install -r tests/requirements.txt
cargo fmt --manifest-path agent/Cargo.toml --check
cargo clippy --manifest-path agent/Cargo.toml --all-targets --locked -- -D warnings
PM_TEST_PYTHON="$PWD/.venv/bin/python" cargo test --manifest-path agent/Cargo.toml --locked
.venv/bin/python -m unittest discover -s tests -v
```

Protocol fixtures bind to loopback and require permission to create local listening sockets. They do not call paid model services. Test-only records are created in temporary directories.

Implementation references: [OpenAI function calling](https://developers.openai.com/api/docs/guides/function-calling), [Responses streaming](https://developers.openai.com/api/docs/guides/streaming-responses), [Rust MCP SDK](https://github.com/modelcontextprotocol/rust-sdk), [Ratatui backends](https://ratatui.rs/concepts/backends/), [OpenWiki](https://github.com/langchain-ai/openwiki), [lopdf PDF extraction](https://docs.rs/lopdf/0.45.0/lopdf/struct.Document.html), [ZIP package reader](https://docs.rs/zip/9.0.4/zip/read/struct.ZipArchive.html), [XML parser](https://docs.rs/roxmltree/0.21.1/roxmltree/struct.Document.html), and the existing [OKF profile](../90_Agent/OKF-Profile.md).
