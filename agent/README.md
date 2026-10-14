---
type: "Reference"
title: "PM Agent application"
language: "en"
---

# PM Agent application

A working first implementation of the local Rust terminal agent. It browses the existing Obsidian workspace, runs its project-management tools, and uses a configured model to read evidence, ask questions and write sourced drafts. macOS and Linux are the target platforms. See [implementation status](Implementation.md) for completed work, tests and remaining milestones.

## Build and start

Run from the framework repository root. Rust 1.96+ and a C toolchain are required. The application and SQLite are compiled into the binary. Built-in PM operations currently use the existing Python 3.10+ implementation embedded in the binary; they are not yet a native Rust port.

```sh
python3 -m venv .venv
.venv/bin/python -m pip install -r requirements.txt -r requirements-pdf.txt
cargo build --manifest-path agent/Cargo.toml --locked
agent/target/debug/pm-agent --vault . tui
```

Browsing, searching and opening documents do not need an API key or model. Press `i`, type `/help`, and press Enter. Edit project content in Obsidian or use `o` to open the selected document with the desktop's associated application. Use `r` to reload changes.

Use a dedicated project copy for actual work. The framework checkout contains no demonstration requirements, staffing or dates. Keep [Project](../00_Governance/Project.md) and [Settings](../00_Governance/Settings.md) authoritative; a conversation's language does not change the document language.

## Configure a model

```sh
mkdir -p .pm-agent
cp agent/config.example.toml .pm-agent/config.toml
```

Edit `.pm-agent/config.toml` locally. Select the provider profile, its actual model ID and endpoint. Set `python` to the absolute path of the virtual environment's Python executable. Model IDs are deliberately empty in the distributed configuration. Keys are read from named environment variables; do not put key values into the config, project documents or chat. Start the application from a shell that has those variables set.

| Profile | Default protocol | Endpoint convention | Verification status |
|---|---|---|---|
| OpenAI | Responses | `https://api.openai.com/v1` | Protocol fixture tested; live account/model pending |
| Ollama | Chat Completions | `http://127.0.0.1:11434/v1` | Protocol fixture tested; installed model pending |
| vLLM | Chat Completions | `http://127.0.0.1:8000/v1` | Protocol fixture tested; model/parser configuration pending |
| CLIProxyAPI | Chat Completions | configurable local gateway | Protocol fixture tested; actual gateway pending |
| OpenRouter | Chat Completions | `https://openrouter.ai/api/v1` | Protocol fixture tested; selected route/model pending |

The example local ports are editable configuration examples, not discovered services. Both adapters stream output and preserve function-call/result context. Responses uses local history and requests encrypted reasoning for stateless continuation. Compatible implementations that reject an optional Responses field should use their tested Chat Completions profile; there is no automatic protocol fallback. `tools = true` enables function declarations but does not certify a model's tool capability. Set it to false for text-only assistance.

```sh
agent/target/debug/pm-agent --vault . doctor
agent/target/debug/pm-agent --vault . chat --stage prd "Review the supplied sources and ask about missing requirements."
```

`doctor` checks local configuration, key presence and Python version. It does not send a live model request. Endpoint authentication, route availability, model quality, tool support and usage accounting still require live acceptance tests. HTTPS is required for remote endpoints; HTTP is allowed on loopback.

## Use the terminal workbench

| Control | Action |
|---|---|
| Tab / Shift-Tab | Change project view |
| Up / Down, j / k | Select a document |
| PgUp / PgDn | Scroll document text |
| F2 | Switch scrolling to Agent output; Home/End jump to its beginning/end |
| i or : | Enter a message or command |
| Enter | Submit input |
| Esc | Leave input mode; cancel an active operation |
| r | Refresh documents from disk |
| o | Open selected document in a desktop application |
| q / Ctrl-C | Exit when idle; Ctrl-C requests cancellation when busy |

Views include BRD, PRD/REQ, domain knowledge, systems, questions, people/schedules, backlog, progress/risks, verification and Agent runs. They show real Markdown records and metadata. Empty views remain empty; no progress, people or risk scores are synthesized for presentation.

| Command | Purpose |
|---|---|
| `/search text` | Literal search, including Chinese and code identifiers; `/search` clears it |
| `/stage prd` | Select a workflow capability before starting a session |
| `/pm plan` | Recalculate a staffing forecast from recorded inputs |
| `/pm validate`, `/pm coverage`, `/pm status` | Generate and display actual PM reports |
| `/sessions`, `/resume ID` | List or load durable conversations |
| `/continue` | Explicitly continue a failed, cancelled or completed turn |
| `/approve`, `/deny` | Decide the exact pending external/import operation |
| `/reconcile actual outcome` | Record what happened to an interrupted operation without repeating it |
| `/new` | Start a separate session, retaining the previous one |

Stages are `workflow`, `intake`, `system`, `language`, `brd`, `prd`, `backlog`, `estimate` and `review`. The runtime loads the corresponding existing Skill and Playbook. Slash PM arguments currently split on whitespace; use the CLI for paths containing spaces or more complex arguments. Do not send secrets through either interface.

## Built-in project operations

The Rust application embeds the reviewed Python helper files at build time. It extracts them to a temporary directory and invokes a configured Python executable with an argument vector, without a shell. It does not execute a project's replacement `tools/pm.py` or registered repository code. Language catalogs still come from the project's `tools/locales` directory so reviewed project translations remain usable.

```sh
agent/target/debug/pm-agent --vault . pm --python .venv/bin/python validate
agent/target/debug/pm-agent --vault . pm --python .venv/bin/python repo-add --id service --path /absolute/repository --wiki openwiki --code-root src
agent/target/debug/pm-agent --vault . pm --python .venv/bin/python sync --repo service
agent/target/debug/pm-agent --vault . pm --python .venv/bin/python ingest /absolute/material.pdf
```

Supported operations: settings, ingestion, repository registration/snapshot, evidence capture, work packets, validation, coverage, planning, status, impact, fingerprint, immutable baseline candidates and OKF export/check. See [the tool manual](../tools/README.md) for their contracts. PDF extraction needs the optional PDF dependencies. Images and incomplete PDF/slide extraction retain the original and require reviewed descriptions; this version does not claim full visual understanding or OCR.

O/M/P effort, availability and dates must be supplied. Unknown capacity is not zero. Imported Wiki descriptions, static code observations, actual test results and requested future behavior remain distinct evidence. A source hash check establishes version identity, not semantic correctness.

## MCP tools

Configure `mcp` profiles in the local config using the commented examples. The official Rust MCP SDK handles initialization and transport negotiation for stdio and Streamable HTTP. Stdio commands run only from operator configuration. Their environment is restricted to basic process variables and explicit `env_names`; server stderr is suppressed to avoid disrupting the terminal or persisting accidental secrets.

Only tools listed in `allowed_tools` are exposed to the model. Every model-requested MCP invocation displays its exact tool and arguments for approval. Schemas are checked locally; external schema references are rejected. Tool descriptions/results remain untrusted data. Remote credentials use an environment variable. OAuth discovery, interactive OAuth login, server-requested sampling and elicitation are not implemented in this milestone.

## Clarification, recovery and persistence

`ask_clarification` creates a project-language Question, displays it, and pauses the run. Other calls in the same model response are blocked. A user response is recorded with source and time; the Agent must assess it with `resolve_clarification` before dependent drafting continues. User messages are retained as exact UTF-8 source assets for citation. The current gate conservatively blocks all draft writes in a session with unresolved questions; dependency-specific parallel progress is a later milestone.

Draft writes require typed frontmatter, project language metadata, draft state and current source hashes. Existing documents require the expected content hash. Approved or pinned documents cannot be overwritten. Updates use a temporary file and atomic replacement. The app locks out another PM Agent writer; external editors do not honor that lock, so hash checks reduce conflicts but are not a distributed editing transaction. Avoid editing a file during its Agent write.

`.pm-agent/runtime.sqlite3` contains durable sessions, tool arguments/results and a rebuildable document projection. It is not a disposable cache. Back up the entire `.pm-agent` directory while the app is closed. The directory is ignored by Git and OKF export, as are build artifacts and local environment files. Shared English execution summaries live under `90_Agent/Runs`; raw user-message assets and generated drafts belong to the project copy.

An interrupted tool has an unknown outcome until reconciled. Restarting does not automatically repeat it. Rate limits, timeouts and provider errors are surfaced without retries. Provider profile changes require a new session. Context and step limits stop explicitly rather than silently dropping evidence. A completed Agent turn is not a project approval, semantic verification or release.

## Verification and references

```sh
cargo fmt --manifest-path agent/Cargo.toml --check
cargo clippy --manifest-path agent/Cargo.toml --all-targets --locked -- -D warnings
PM_TEST_PYTHON="$PWD/.venv/bin/python" cargo test --manifest-path agent/Cargo.toml --locked
.venv/bin/python -m unittest discover -s tests -v
```

Protocol fixtures bind to loopback and require permission to create local listening sockets. They do not call paid model services. Test-only records are created in temporary directories.

Implementation references: [OpenAI function calling](https://developers.openai.com/api/docs/guides/function-calling), [Responses streaming](https://developers.openai.com/api/docs/guides/streaming-responses), [Rust MCP SDK](https://github.com/modelcontextprotocol/rust-sdk), [Ratatui backends](https://ratatui.rs/concepts/backends/), [OpenWiki](https://github.com/langchain-ai/openwiki), and the existing [OKF profile](../90_Agent/OKF-Profile.md).
