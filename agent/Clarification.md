---
type: "Reference"
title: "Scoped clarification and independent work"
language: "en"
---

# Scoped clarification and independent work

Missing information stays unknown. The application records which work a question blocks so that supported independent work can continue. This enforces recorded relationships; it cannot determine whether an answer is semantically sufficient.

## Ask and pause

`ask_clarification` persists a project-language question and displays its requested information. It always pauses the run and discards later calls in the same model response, including parallel writes. The user sees the question before any continuation.

New questions use the `clarification-v1` profile:

| Property | Meaning |
|---|---|
| blocking_scope | `project` by default; `records` only when evidence establishes a bounded scope |
| affected_ids | Existing managed document IDs; a missing or ambiguous root makes the gate unavailable |
| affected_paths | Exact managed Markdown paths, including explicitly planned files; no directories or patterns |
| workflow_status | `open` until an actual response has been assessed and resolution recorded |
| owner / due_date | Null until supplied |

Project scope requires empty target lists. Record scope needs at least one target, with at most 100 explicit targets. Question text must explain the scope, inspected evidence and requested materials. Narrowing scope to evade a missing premise is prohibited.

## Continue or answer

In Chat, `/continue-independent` resumes local evidence-backed work without creating an answer asset or resolving a question. CLI: `chat --session SESSION-ID --independent`.

Use `/answer QUESTION-ID TEXT` to associate actual input with one question. CLI: `chat --session SESSION-ID --answer QUESTION-ID "Actual response"`. An exact question path is also accepted. Resume the originating session before answering its question. A manually authored open scoped question without a session can be associated with the current session through this explicit answer action.

When exactly one question is open in a paused session, an ordinary reply answers that question. Multiple open questions require explicit selection. A task message in a session blocked by another session's question does not become that question's answer.

Actual replies are captured as UTF-8 user source assets with their path, SHA256, session locator and time. The response fingerprint covers the question body and original metadata, including scope. It excludes response/status fields. Changing the question, scope or captured response invalidates the old response. Receipt alone leaves the question open. `resolve_clarification` additionally needs a nonempty explanation of how the actual response supplies the missing premise. It never establishes project approval or human verification.

## Gates and compatibility

All versioned questions in the workspace apply to Agent draft writes across sessions. The gate checks explicit targets and recorded downstream Wiki links, glossary contexts, repository/material evidence relationships and exact citation paths. Current and proposed draft references are checked, so removing a link or changing the filename does not erase an existing dependency. Unresolved references in the target draft prevent establishing independence. These are conservative structural checks, not semantic dependency discovery.

An invalid scoped question contract blocks Agent writes until repaired. Baseline capture checks every included document before creating a candidate directory. Generic `write_draft` cannot create or overwrite question records; use the surfaced question workflow.

Local reads, source analysis and deterministic reports remain available. MCP tool invocations are blocked while open questions remain because their independence cannot be established from local record paths. The connection may still initialize and discover its configured tools. Once the questions are resolved, the normal exact-call MCP decision applies.

Legacy questions without this profile retain the original session-wide draft block. No cross-session scope or answer fingerprint is invented for them. Existing serialized sessions remain readable. Human editing in Obsidian remains available; repairing a question requires preserving actual provenance. The application uses optimistic file checks, not a filesystem transaction or tamper-proof audit.

## Browse questions

F9 or Questions in a document context menu opens a read-only view of scopes, affected documents, captured responses and resolution checks. Enter focuses details; Esc returns to the list and then Browse. O or right-click opens the note. R refreshes; mouse wheel and the scrollbar navigate Markdown details. F6 toggles emoji. Existing chat drafts remain intact.

`pm questions` and `/pm questions` expose the same current data without contacting a model or creating a report. Background refresh includes explicitly cited user-answer assets; invalid or changed responses become visible. No question is resolved by browsing.

See [schema](../00_Governance/Schema.md), [question template](../80_Templates/Question.md), [evidence policy](../90_Agent/Evidence-and-Clarification-Policy.md) and [application usage](README.md).
