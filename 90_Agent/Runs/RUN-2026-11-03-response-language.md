---
id: "RUN-2026-11-03-RESPONSE-LANGUAGE"
type: "agent_run"
title: "Configurable default Chat response language"
language: "en"
workflow_status: "completed"
---

# Configurable default Chat response language

## Request and effective policy

The user requested a selectable default language for Agent replies in Chat, included in the starting system prompt. Source: the direct user message in this development conversation on 2026-11-03. Project document_language is en; agent_language and this operational record use en. Conversational response language is a separate operator preference.

## Inputs and changes

Starting revision: ae8c31c on feat/rust-pm-agent. Read AGENTS.md, Settings, Language Policy, OKF Profile, Project, CONTEXT-MAP, application documentation, configuration, runtime, provider system-message assembly and TUI dispatch/completion.

- agent/src/config.rs: optional top-level response_language and bounded tag validation, with backwards-compatible omission.
- agent/src/runtime.rs: snapshot the preference into new sessions, add the instruction to the system prompt and report the selection in operational Runs. Legacy sessions retain automatic replies.
- agent/src/tui.rs, agent/src/tui/view.rs and agent/src/tui/completion.rs: local /language inspection/selection, suggested tags, session protection, status/help. Selection persists across /new until TUI exit; config.toml supplies the persistent default for both TUI and CLI Chat.
- agent/config.example.toml and agent/README.md: configuration examples, scope, automatic replies and resume behavior.
- agent/tests/integration.rs: exercise both wire protocols, initial and resumed prompts, changed defaults and legacy sessions; update its Config constructor. agent/tests/evaluation.rs and agent/tests/probe.rs: update Config constructors.

The prompt permits explicit conversational requests for another language, while preserving document_language, English operational records, quotations and technical identifiers. No project inputs, provider credentials or approved baselines changed.

## Checks and evidence

- cargo test --manifest-path agent/Cargo.toml --locked --lib --test integration: 114 unit tests and 42 integration tests passed, including configuration validation, local TUI selection and both provider system prompts. Loopback fixtures required execution outside the restricted network sandbox; no live model requests were made.
- cargo clippy --manifest-path agent/Cargo.toml --all-targets --locked -- -D warnings: passed.
- cargo fmt --manifest-path agent/Cargo.toml --check and git diff --check: passed.
- Native pm validate: 0 errors, 1 PROJECT_SETUP warning because the starter charter remains unconfigured; no project scope or commitments were invented. Generated report: 90_Agent/Reports/Quality-Report.md.

These fixture checks verify request construction and persistence, not a live model's linguistic quality or instruction adherence. First compilation downloaded locked dependencies. An initial offline attempt lacked the dependency cache; the restricted loopback attempt could not bind a socket, then passed with local-network access.

## Questions and pending decisions

No clarification or external material was required. No dependent work is on hold. No live provider or manual terminal acceptance is claimed. The user subsequently authorized committing and pushing this change to the existing feat/rust-pm-agent branch.
