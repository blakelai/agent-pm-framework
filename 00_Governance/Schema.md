---
type: "Reference"
language: "en"
title: "Project document contract"
---

# Project document contract

Read Settings and Language Policy before authoring. Flat Obsidian Properties hold stable identifiers and machine fields; detailed rules and tables belong in Markdown. Project IDs are stable application identifiers, separate from OKF path-based concept IDs.

| Object | Core fields | Meaning |
|---|---|---|
| source | id, type, workflow_status, review_status, sha256, asset | Original-material review is separate from business approval |
| evidence | id, type, workflow_status, locator, assertion_type, source_sha256 | Set workflow_status:verified only after source review |
| brd / prd / requirement | id, type, version, doc_status, owner, contexts, evidence, language | Business intent, product behavior and atomic assertions |
| glossary | id, type, context, version, doc_status, language | Authoritative meaning within one bounded context |
| pbi | id, type, work_type, delivery_status, owner, contexts, sprint, depends_on, language | Executable work; sprint may be null |
| testcase | id, type, requirements, language | Test design, separate from execution |
| test_run | id, type, cases, result, executed_at, executed_by, environment, build, evidence, requirement_versions | Actual execution evidence |
| release | id, type, workflow_status, items, test_runs, baseline_id | Release scope and outcomes |
| risk / issue / action / question | id, type, workflow_status, owner, due_date | Open management follow-up |
| decision | id, type, workflow_status, decided_by, decided_at, decision_evidence | Decisions that actually occurred |

`doc_status` is draft, in_review, approved or retired. `version` is a positive integer. `delivery_status` is proposed, ready, in_progress, blocked, done or cancelled. `workflow_status` carries other project-process states; OKF `status`, when supplied, only describes knowledge lifecycle. Structural checks do not enforce an unbypassable approval system.

| work_type | Required origin | Completion basis |
|---|---|---|
| feature / migration | parents: PRD; satisfies: REQ | AC and DoD, plus reconciliation/rehearsal for migration |
| bug | origins: issue or requirement | Reproduction, fix and regression evidence |
| enabler | origins: system analysis, design, decision, requirement or question | Inspectable technical outcome |
| spike | origins: question; research_question; positive timebox_days | Timeboxed findings, evidence, options and next step |

Every PBI has numbered ACs. Ready work records ready_by, ready_at and readiness_evidence. Executing feature/migration PBIs pin a baseline containing applicable PRD, REQs and glossaries. Done work records acceptance_evidence and dod_evidence; feature/migration/bug work maps each AC with `AC-01=TC-001` entries in ac_coverage.

A requirement is one verifiable assertion, linked to its PRD and evidence, with kind FR, NFR, INT, DATA or TR. A test case links requirements. A test run records `ID@version:hash` tokens from fingerprint; simulation:true never qualifies as an actual pass. Source review verifies fidelity, not business approval.

See [baseline/change procedure](Baselines-and-Changes.md), [Planning](../06_Delivery/Planning.md) and [OKF profile](../90_Agent/OKF-Profile.md).

Working tools use flat Properties and JSON-compatible inline lists/maps. Keep optional nested OKF families in JSON-style inline values when using these tools; the export validator accepts standard YAML, but the working reader is not a general-purpose OKF consumer.
