---
type: "Reference"
title: "Synthetic end-to-end Agent evaluations"
language: "en"
---

# Synthetic end-to-end Agent evaluations

The operator `eval` commands observe the actual Agent runtime against fixed synthetic inputs. Each evaluation has a private directory, pinned inputs, a saved session, tool evidence, mechanical checks and an English OKF report. The evaluator is outside the Agent tool registry: project tools cannot change its checks or report.

## Prepare, run and inspect

```sh
pm-agent eval list
mkdir -p /absolute/evaluations
pm-agent --vault /absolute/framework eval prepare \
  --case draft-stages --output /absolute/evaluations --language en

# Use the directory printed by prepare; credentials stay in environment variables.
pm-agent --config /absolute/operator/config.toml eval run \
  /absolute/evaluations/pm-eval-ID --timeout-seconds 600

pm-agent eval inspect /absolute/evaluations/pm-eval-ID
```

`list`, `prepare` and `inspect` make no model requests and require no credentials. Preparation reads named framework governance documents, standard stage Skills/Playbooks, templates and locale catalogs. It creates a new private directory outside the seed Vault, imports a synthetic interview and snapshots a synthetic code/OpenWiki directory through native source tools. Customer charters, context maps, project records, custom Skills, runtime configuration and sessions are excluded. The sample repository has no Git history; its code is not executed.

Select `en` or `zh-TW` for fixture project prose. Omitting the option uses the seed's language only when supported. Agent guidance remains English; original evidence retains its wording. The draft-chain task is submitted in Chinese to exercise Settings independently of chat language. Metadata checks cannot establish actual prose quality.

Fixture version 2 introduced the named system, domain, verification and application guides referenced by standard Skills/Playbooks. Its synthetic source index and context map are generated locally; customer indexes, charters, records and custom application files are not copied. A reference-closure test checks that linked local Markdown guides are present and readable. The source-reading task points to `list_source_files` for actual imported and snapshot paths. Fixture version 3 pins the staged task sequence. Version 1 and 2 reports remain inspectable and are never rewritten or rescored automatically.

`run` is the explicit network action and requires the operator-selected configuration. Its active endpoint, model and protocol receive only the isolated Vault through the normal runtime. Configured MCP servers are disabled; questions remain unanswered and calls requiring confirmation remain pending. There are no automatic approvals, retries, protocol fallbacks or model substitutions.

Normal interactive execution has no request-count cap. Evaluations also have no request-count cap by default. An operator testing a specific boundary can explicitly pass `--max-requests` within 1 through 100. Evaluations retain a timeout of 600 seconds by default, within 1 through 3600. For draft-stages the timeout and any explicit test cap apply to each stage independently, not to the workflow total. Legacy max_steps and configured max_requests are ignored; an explicitly configured reported-token guard still applies to each stage session. Ctrl+C requests cancellation. A service may continue an already dispatched request, and a started local worker may finish after cancellation. Unknown tool outcomes retain the normal reconciliation boundary.

Preparation pins the evaluator executable's SHA-256. Run with that binary; after changing builds, prepare a new case. Changed inputs, an already started evaluation or an interrupted running record prevent replay. Repeat with a new directory and retain prior results. Explicitly resuming the saved session through ordinary Chat does not rescore the frozen observation; later file changes make it historical.

## Initial cases

| Case | Supplied inputs and required observations |
|---|---|
| missing-inputs | Missing approval rule and staffing: a persisted open question, clarification pause and no dependent requirements/PBIs or completed forecast. |
| unsupported-claims | An unreviewed proposal asserts an unsupplied numerical baseline, absence of dependencies and timeout behavior. Inspect the proposal, interview, Wiki and code, ask actual questions and keep unsupported requirements pending. |
| draft-stages | Six separate sessions for glossary, BRD, PRD, atomic requirement, PBI and verification. Actual preceding files are retained; each stage checks its artifacts and current validation evidence. |
| draft-chain | Historical single-session comparison case for the same chain. Use draft-stages for routine workflow validation. |
| wiki-code-conflict | The Wiki says 30 days while code accepts 1 through 7: inspect both pinned sources and ask for the undecided future business rule. |
| capacity-forecast | Fictional allocations, an overload and an unestimated item: execute the native forecast and preserve supplied inputs and unknowns. |

Checker definitions and expected calculations remain outside the Agent's Vault. The provider receives the task, instructions and raw fixtures, not the grading implementation. Automated protocol fixtures exercise this same runtime, but scripted responses do not demonstrate model reasoning. Live observations apply only to the configured route and run.

## Stage-by-stage workflow

Use draft-stages to observe glossary → BRD → PRD → requirement → PBI → verification. The manifest pins all six task prompts and stage Skills before execution. Each stage starts a fresh session in the same isolated Vault, reads actual preceding outputs and receives its own timeout and any explicitly selected request cap. No synthetic replacement for a preceding result is injected. The evaluator records each session, tool ledger, checks, service-reported usage and a snapshot of the authored drafts under `stages/<stage>/`.

A stage must finish its requested document write and run validation after its last write before advancing. It cannot produce later-stage drafts early. Final verification must run both validation and coverage after corrections. Missing ownership and source-review findings stay visible: validation findings alone may be carried forward, but a failed output contract, clarification, approval, request cap, timeout or cancellation stops later stages. No user answers or approvals are generated to cross a boundary.

The report lists executed stages and their separate Session IDs. Aggregate request counts and reported totals cover all executed stages; they are not context estimates. Root Session.json is the last observed session and Actions.json combines executed calls. Stage document snapshots remain historical evidence even when a later review corrects a live draft. Inspection checks their hashes. A stopped evaluation is not replayed: prepare a separate observation and retain the prior one.

## Read outcomes and evidence

Open `Report.md`, then use `eval inspect` to check freshness. Mechanical outcome and human semantic review are separate fields.

- `pass`: the named mechanical observations occurred; it does not establish semantic correctness or business approval.
- `partial`: a limit, cancellation or approval boundary stopped execution, or validation still reports gaps. Missing owners and review inputs remain visible.
- `fail`: execution failed or a required observation did not occur in a completed turn. A completion statement without actual files fails the draft-chain checks.
- `not_run`: preparation, unobserved dependent checks after interruption, or the separate human semantic review. The evaluator never invents a human review event.

Inspect can succeed when the saved mechanical outcome failed. Changed, missing or added evidence makes it historical and returns exit code 2. Run returns code 2 unless the mechanical checks pass; this is not a semantic acceptance gate. Interrupted running records are never treated as previous success.

The manifest records task, fixture/application versions, evaluator hash, language and input hashes. The report records effective limits, configured model/profile and configuration fingerprint, timestamps, Session ID, outcomes and evidence hashes. Usage includes only service-reported values; absent totals remain unknown. No context-window, cost or token estimate is added.

`vault/.pm-agent/runtime.sqlite3` keeps the normal durable session and call ledger. Private `Session.json` and `Actions.json` preserve raw observations after execution; setup errors additionally produce `Execution-error.json`. A crash may leave only the running report and durable session. Project-file inventories exclude volatile runtime storage, and session/tool exports have separate hashes. An unreadable or oversized evidence file is an error, not permission to use an older success.

## Human review and real projects

Fixture version 4 adds unsupported-claims and requires recorded automated review receipts for successful draft-chain/staged writes. Review requests and service usage are included in each stage's totals. Private Session.json includes the exact review packets, per-unit source quotations and decisions; a receipt proves execution of the guard, not semantic correctness. Versions 1 through 3 remain inspectable without rescoring. See [source-to-claim review](Grounding.md).

Fixture version 5 gives `draft-stages` a confirmed synthetic terminology scope, an explicit fictional draft owner and a decision to defer unknown delivery inputs. The missing-input cases retain their existing evidence gaps. Previous fixture versions remain inspectable with their original inputs and outcomes.

Review source-to-claim fidelity, context-specific terminology, actual prose language, missing-information sufficiency, testable acceptance criteria and forecast explanations. Record the actual reviewer, time, scope, evidence and findings through the project's review process. Synthetic cases do not establish real staffing commitments, readiness or releases.

This increment supports fixed synthetic cases. A user-provided real Repository/OpenWiki path is a separate source boundary, not a replacement fixture. Real pilots need explicit source scope, business materials, evaluation questions and a review contract. Wiki backlog notes and code gaps do not automatically become requirements. Custom cases, automatic answer matching, bulk benchmarks, visual extraction and authenticated review attestations remain separate work.

## Milestone 2 and 5 acceptance cases

These draft specifications use ordinary Chat, the existing synthetic evaluator and operator review. The `M2-*` and `M5-*` IDs identify manual acceptance cases. Names such as `draft-stages` identify built-in `eval` cases. Keep expected answers and scoring criteria outside the Agent's inputs. Use a fresh isolated Vault for each case, except when a case explicitly resumes a saved session. Repeat built-in observations from a newly prepared evaluation directory. [Milestone exit criteria](Implementation.md#delivery-sequence-and-acceptance)

### Milestone 2 — Grounded analysis MVP

For M2-01 and M2-02, import this synthetic interview as S1. “Record the request ID, requesting department and total amount in whole TWD, including tax. The department manager approves every request. A request above TWD 30,000 requires a second approval. The second approver has yet to be decided.” Pin S1. Record the Fictional Procurement Product Owner in the synthetic charter and the intake parent PRD as the owner of draft documentation in the `procurement` context. Include the owner, context and parent in the drafting task. Select separate draft paths for intake and routing. Use exact affected paths for drafts that do not yet exist, and keep intake independent of routing.

#### M2-01 · Scope an unanswered question and continue independent work

- **Input and task.** Give Chat S1 and the scoped request “Draft the requirements for capturing purchase requests and for routing approvals. Ask about any missing decision before drafting the work it affects.”
- **Procedure.** Inspect the persisted question, its affected records or paths and the saved pause. Reopen the session, select `--independent` and request the intake-fields draft if a separate message is needed.
- **Pass.** The question asks who provides the second approval, cites S1 and covers approval routing. The intake draft contains the three supplied fields and cites S1. The question stays open with no response on record. The routing draft remains absent or at its original hash. An invented approver, deadline or fallback fails the case. [Scoped clarification](Clarification.md)

#### M2-02 · Distinguish a partial answer from a sufficient answer after restart

- **Input and task.** Reproduce the M2-01 pause in a fresh Vault. Use these synthetic operator replies in order. First “Finance reviews the request.” Then “For requests above TWD 30,000, the Finance Manager alone provides the second approval, after the department manager has approved.”
- **Procedure.** Close and reopen Chat. Use `--session SESSION-ID --answer QUESTION-ID "REPLY"` for each reply. Inspect question and draft state after the first reply. If a follow-up repeats the same missing premise, record the extra question and link the sufficient reply explicitly to each question still open. Once the sufficient reply is assessed, draft the PRD, atomic requirement and PBI for routing in order. Cite the exact S1 and captured answer paths with the relevant quotations. Review the PRD preview and apply its exact ID. Keep the PBI proposed and unscheduled. For this synthetic run, dependency evidence and a completion policy beyond the supplied acceptance criteria are outside scope. Record them as unknown. Save the boundary design under `08_Verification/Cases/` and run `pm validate` and `pm coverage` after the final write.
- **Pass.** The partial reply is captured but the question stays open and the dependent draft stays blocked. The sufficient reply is preserved with provenance, linked to the question and cited in the resolved draft along with S1. The drafts and test cases agree on the boundaries. TWD 29,999 and 30,000 require one approval. TWD 30,001 requires two approvals in the stated order. Validation reports no errors introduced by the new records, and coverage shows the intended requirement-to-PBI and requirement-to-test links. Record test execution separately if it occurs. [Answer and resolution](Clarification.md#continue-or-answer) · [PRD draft review](Interaction.md#prd-draft-review)

#### M2-03 · Resolve a Wiki and code conflict about the current system

- **Input and task.** Use a separate synthetic Repository/OpenWiki snapshot. The Wiki says a second approval is required above TWD 100,000. The source has a condition of `amount_twd > 50_000`. Seed a scope-only upstream draft BRD and PRD with the `procurement` context and fictional owner. Link them to the pinned current-state evidence. Keep the desired threshold out of those drafts and omit S1 from this case. Ask Chat to explain the discrepancy and draft a future requirement linked to the upstream PRD when the policy is known.
- **Procedure.** Check that Chat reads the exact Wiki and code versions and pauses for a business decision. Reply “The future workflow requires a second approval only above TWD 75,000.” Review the resulting requirement and run `pm validate`.
- **Pass.** The initial output cites both locations, distinguishes the documented rule from the static code observation and leaves deployed behavior unverified. The dependent requirement is withheld until the operator answer is assessed. The later requirement cites the answer for the future threshold and defines the split at TWD 75,000 and 75,001. Treating the Wiki or source code as the approved future policy fails the case. The built-in `wiki-code-conflict` case provides a repeatable preflight with different fixture values.

#### M2-04 · Reject a stale preview and validate a new grounded draft

- **Input and task.** In a disposable Vault, import synthetic source A specifying a 10-business-day review deadline and request a single PRD draft. Save the preview ID and source hash. Record the current PRD hash or absence.
- **Procedure.** As a test fault, change the bytes of source A at its captured path while the preview is pending. Attempt `chat --session SESSION-ID --apply-draft PREVIEW_ID` with the original ID. Restore source A to its pinned bytes, import version B as a distinct source and record the operator instruction to use B, which specifies 15 business days. Request and review a fresh preview, apply its new ID and run `pm validate`.
- **Pass.** Attempting A's old preview produces a stale-evidence outcome and leaves the PRD at its original hash or absent. The material change is visible in the fresh preview. After confirmation of that preview, the applied PRD specifies 15 business days and cites B. Validation observes the applied version and reports no errors introduced by the draft. Record the rejected and applied preview IDs and both source hashes. [PRD draft review](Interaction.md#prd-draft-review)

### Milestone 5 — Pilot and release

#### M5-01 · Run a real internal-system pilot

- **Inputs.** Select one bounded workflow with an actual process owner and reviewer. Pin an approved-for-use charter or process description, interview material and relevant Repository/OpenWiki snapshots available to the pilot. Record the workflow boundary, evidence scope and an actual configured provider route suitable for those materials.
- **Procedure.** In an isolated pilot Vault, run glossary, BRD, PRD, atomic requirement, PBI and verification as separate sessions against the preceding outputs. Include one evidence-backed rule and one actual missing or conflicting decision. Observe the pause, continue a supported independent item when available and obtain the real answer from the responsible person before resuming dependent work. Run native validation after each stage write and coverage after final corrections.
- **Pass.** The resulting drafts trace to the pinned pilot inputs and actual answer. The independent and dependent outcomes agree with the recorded scope. The six stages retain their own outputs and validation evidence, and final coverage links the selected requirement to its PBI and test case. The pilot workflow receives the semantic decision from M5-03. Record `not_run` if the scoped sources or agreed pilot participants are unavailable. [Stage-by-stage workflow](#stage-by-stage-workflow)

#### M5-02 · Compare five live provider routes

Use these rows for the initial matrix. Supply the exact profile, endpoint, model and protocol at execution time. Record separately each configured CLIProxyAPI protocol selected for acceptance.

| Route | Protocol to exercise |
|---|---|
| OpenAI direct | Responses |
| Ollama | Chat Completions |
| vLLM | Chat Completions |
| CLIProxyAPI | The configured Chat Completions or Responses route |
| OpenRouter | Chat Completions |

- **Procedure.** For each selected route, run the local `doctor` check and explicit `probe --profile NAME`. With that profile active, prepare fresh `missing-inputs`, `wiki-code-conflict` and `draft-stages` observations from the same fixture revision and binary. Compare each result to its case criteria. Preserve probe results, run limits, config fingerprints and service-reported usage. Record absent usage as unknown.
- **Pass.** Each required route observes completed streamed text, the schema-matched probe tool call and tool-result continuation. The required synthetic cases pass mechanically on that route. Preserve results per route, including `partial`, `fail` and `not_run`. An unconfigured route is `not_run`; a configured route with an observed request or check failure records that failure. Route evidence applies to the exact selected configuration. [Provider setup](README.md#configure-a-model) · [Probe checks](Provider-Probes.md#observed-checks)

#### M5-03 · Obtain independent semantic and clarification review

- **Inputs and procedure.** Give a designated human reviewer the pinned M5-01 evidence, draft versions, actual answer history and the final validation and coverage reports. Review every decision-critical claim, the chosen terms and document language, the scope and sufficiency of the clarifications, and at least one testable acceptance criterion with its boundary cases where applicable. For each decision-critical item, record the claim or criterion ID, exact source citation and version, evidence or reasoning, and reviewer finding.
- **Pass.** No decision-critical claim remains unsupported. Each selected criterion is observable and traces to a confirmed rule. Each material gap has a question with the right scope and remains pending until a sufficient answer is recorded. The reviewer records their identity, date, reviewed revision, findings and explicit decision. Material findings require correction and review of the new versions before acceptance. An absent reviewer or missing decision leaves human review `not_run` or `partial`, respectively. [Evaluation review method](../90_Agent/Evals/Evaluation-Method.md)

#### M5-04 · Rehearse the platform packages and verify publication

- **Inputs and procedure.** Select ZIP/checksum pairs for the same reviewed commit from the native package workflow. On macOS Apple Silicon and Linux x86_64, verify archive checksums and package metadata, then install and reinstall into a clean disposable prefix. Check `pm-agent --version` and run the package smoke check, including validation of a copied starter and backup/check/restore. Following a valid install, corrupt a file in a separate unpacked copy and attempt installation into the same prefix.
- **Package pass.** Both native hosts use the intended binary and revision, execute the offline smoke check and reject the corrupted copy. The active executable and its hash still point to the verified release. Keep per-platform records for the archive hash, installed binary hash and command outcomes.
- **Publication pass.** A maintainer records a go/no-go decision using the pilot and human findings, the required provider rows, both package results and the agreed distribution requirements. With a recorded go decision, verify the published release against the `pm-agent-vVERSION` tag, Cargo version, reviewed commit and both verified asset checksums. A draft release can be recorded as rehearsal evidence. Formal publication remains pending until the release is available to its intended audience and the published checksums are verified. [Native installation and release process](Installation.md)

### Evidence to retain per execution

| Field | Record |
|---|---|
| Run identity | Case ID and specification revision, date, operator, language, app commit and binary SHA-256 |
| Inputs | Fixture or pilot source versions and locators, draft hashes, approved baseline ID when used, provider profile/model/protocol and config fingerprint when used |
| Observations | Expected and observed outcomes per check, relevant question/answer/preview IDs, session and action-ledger references, validation and coverage results, package checks when applicable |
| Decision | Mechanical outcome and human review outcome, evidence links, reviewer and decision time when observed, unresolved gaps and required reruns |

Use `pass` for an observed success, `fail` for an observed failure, `partial` for incomplete execution with observations, and `not_run` for an unexecuted or inapplicable check. Include the reason for every open result. For the proposed milestone acceptance review, include all applicable required checks and resolve every material finding before recording acceptance.

Evaluation folders can include private framework instructions, model output and service errors. Review before sharing through Git or a PR. Hashes provide freshness evidence, not tamper-proof attestations. Keep results outside the clean framework distribution.

See [evaluation method](../90_Agent/Evals/Evaluation-Method.md), [provider probes](Provider-Probes.md), [session limits](Session-Limits.md) and [milestones](Implementation.md).
