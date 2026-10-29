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
  --case missing-inputs --output /absolute/evaluations --language en

# Use the directory printed by prepare; credentials stay in environment variables.
pm-agent --config /absolute/operator/config.toml eval run \
  /absolute/evaluations/pm-eval-ID --max-requests 32 --timeout-seconds 600

pm-agent eval inspect /absolute/evaluations/pm-eval-ID
```

`list`, `prepare` and `inspect` make no model requests and require no credentials. Preparation reads named framework governance documents, standard stage Skills/Playbooks, templates and locale catalogs. It creates a new private directory outside the seed Vault, imports a synthetic interview and snapshots a synthetic code/OpenWiki directory through native source tools. Customer charters, context maps, project records, custom Skills, runtime configuration and sessions are excluded. The sample repository has no Git history; its code is not executed.

Select `en` or `zh-TW` for fixture project prose. Omitting the option uses the seed's language only when supported. Agent guidance remains English; original evidence retains its wording. The draft-chain task is submitted in Chinese to exercise Settings independently of chat language. Metadata checks cannot establish actual prose quality.

Fixture version 2 includes the named system, domain, verification and application guides referenced by standard Skills/Playbooks. Its synthetic source index and context map are generated locally; customer indexes, charters, records and custom application files are not copied. A reference-closure test checks that linked local Markdown guides are present and readable. The source-reading task points to `list_source_files` for actual imported and snapshot paths. Version 1 reports remain inspectable and are never rewritten or rescored automatically.

`run` is the explicit network action and requires the operator-selected configuration. Its active endpoint, model and protocol receive only the isolated Vault through the normal runtime. Configured MCP servers are disabled; questions remain unanswered and calls requiring confirmation remain pending. There are no automatic approvals, retries, protocol fallbacks or model substitutions.

The default request cap is 32, within 1 through 100; a lower configured session cap still applies. Existing reported-token limits and `max_steps` remain effective. The overall timeout defaults to 600 seconds, within 1 through 3600. Ctrl+C requests cancellation. A service may continue an already dispatched request, and a started local worker may finish after cancellation. Unknown tool outcomes retain the normal reconciliation boundary.

Preparation pins the evaluator executable's SHA-256. Run with that binary; after changing builds, prepare a new case. Changed inputs, an already started evaluation or an interrupted running record prevent replay. Repeat with a new directory and retain prior results. Explicitly resuming the saved session through ordinary Chat does not rescore the frozen observation; later file changes make it historical.

## Initial cases

| Case | Supplied inputs and required observations |
|---|---|
| missing-inputs | Missing approval rule and staffing: a persisted open question, clarification pause and no dependent requirements/PBIs or completed forecast. |
| draft-chain | A narrow access-duration rule: linked glossary, BRD, PRD, atomic requirement and PBI drafts, current source citations, unknown unscheduled effort, validation and coverage. |
| wiki-code-conflict | The Wiki says 30 days while code accepts 1 through 7: inspect both pinned sources and ask for the undecided future business rule. |
| capacity-forecast | Fictional allocations, an overload and an unestimated item: execute the native forecast and preserve supplied inputs and unknowns. |

Checker definitions and expected calculations remain outside the Agent's Vault. The provider receives the task, instructions and raw fixtures, not the grading implementation. Automated protocol fixtures exercise this same runtime, but scripted responses do not demonstrate model reasoning. Live observations apply only to the configured route and run.

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

Review source-to-claim fidelity, context-specific terminology, actual prose language, missing-information sufficiency, testable acceptance criteria and forecast explanations. Record the actual reviewer, time, scope, evidence and findings through the project's review process. Synthetic cases do not establish real staffing commitments, readiness or releases.

This increment supports fixed synthetic cases. A user-provided real Repository/OpenWiki path is a separate source boundary, not a replacement fixture. Real pilots need explicit source scope, business materials, evaluation questions and a review contract. Wiki backlog notes and code gaps do not automatically become requirements. Custom cases, automatic answer matching, bulk benchmarks, visual extraction and authenticated review attestations remain separate work.

Evaluation folders can include private framework instructions, model output and service errors. Review before sharing through Git or a PR. Hashes provide freshness evidence, not tamper-proof attestations. Keep results outside the clean framework distribution.

See [evaluation method](../90_Agent/Evals/Evaluation-Method.md), [provider probes](Provider-Probes.md), [session limits](Session-Limits.md) and [milestones](Implementation.md).
