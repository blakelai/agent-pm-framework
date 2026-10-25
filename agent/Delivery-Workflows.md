---
type: "Reference"
title: "Delivery workflow views and evidence gates"
language: "en"
---

# Delivery workflow views and evidence gates

F7 or Delivery in Browse opens four local views: Changes, UAT, Releases and Benefits. The view reads actual project records and shows their recorded state, structural problems, the first unresolved next-gate check and supplied fields. Empty projects remain empty. Opening or refreshing this view makes no model call and writes no report.

## Controls and editing

Use 1–4, Alt+1–4 or the tab labels to select a workflow. Up/Down selects a record; Enter focuses its Markdown details and Esc returns to the list. Esc from the list closes the view. Tab changes focus. Mouse wheel scrolls the pointed pane; the right scrollbar supports click and drag. N creates a draft for the selected workflow, E or right-click on a record opens supported fields, O opens its document in the main browser, and R reloads current records. The footer provides clickable actions. The chat draft is preserved.

The [structured editor](Editing.md) asks for a stable ID, project-language title and content. No business content, actor, event time, estimate or measurement is filled automatically. Ctrl+S checks and previews the change; a separate Ctrl+S saves it and writes an English edit record. Changes in the document or project evidence invalidate an earlier preview. Keep incomplete records as drafts and request missing material before recording dependent outcomes. F6 toggles icons without changing record data.

## Versioned record contract

New records carry workflow_schema: delivery-v1. This local application extension uses the existing [OKF profile](../90_Agent/OKF-Profile.md), preserves process state in workflow_status, and does not change OKF knowledge status. Author project content in document_language. Templates remain English blueprints.

| Type | Location for TUI creation/editing | States |
|---|---|---|
| change | 07_Decisions/Changes | draft, in_review, accepted, rejected |
| uat | 08_Verification/UAT | draft, in_review, blocked, accepted, rejected |
| release | 06_Delivery/Releases | draft, in_review, ready, released, cancelled |
| benefit | 09_Operations/Benefits | draft, observing, measured |

Drafts may omit inputs. Supplied values and links must still be well formed. Each review/outcome needs an owner; blocked UAT may remain incomplete. Event timestamps in this profile require RFC3339 with an explicit timezone. Reference fields use lists of resolvable typed documents with IDs; self references, duplicates and simulated references are rejected. previous_records links earlier records of the same type. Generic evidence links establish inspectability, not truth or organizational authorization.

Accepted/rejected changes and UAT, released/cancelled releases, and measured benefits are historical in the editor. Create a new linked record to correct them; nothing automatically supersedes, rescinds or rewrites an earlier outcome. Baseline approval still follows the separate [baseline procedure](../00_Governance/Baselines-and-Changes.md).

A ready release keeps its reviewed scope, content and go decision fixed. The editor permits only release-event fields and a transition to released. Changed scope or a subsequent no-go decision requires a linked new record. This protects the recorded review basis inside the editor; external files are still user-editable and are not a signed approval system.

UAT also reuses the existing PBI readiness/completion checks, including ownership, traceability, baseline contents, AC mappings and acceptance/DoD evidence. A done label alone does not satisfy the gate.

## Workflow checks

| Outcome | Required supplied information |
|---|---|
| Change accepted or rejected | owner, affected_records, impact_evidence, decided_by, decided_at, decision_evidence; an optional baseline_id must refer to an approved baseline |
| UAT accepted | owner, approved baseline_id, completed items, matching requirements, selected cases covering every requirement, current passing test_runs for the pinned versions, reviewed_by/at and decision_evidence |
| UAT rejected | owner, reviewed_by/at and decision_evidence; a passing result is not required to record rejection |
| Release ready | owner, approved baseline_id, done items, accepted delivery-v1 UAT on the same baseline, identical aggregate item/test scope, cutover_plan, rollback_plan, handover, actual go decision with actor/time/evidence |
| Release released | All ready checks plus released_by/at and release_evidence; release time must follow the go decision |
| Release cancelled | owner, actual no_go decision with actor/time/evidence |
| Benefit observing | owner, metric, unit, measurement_definition, numeric baseline/target, data_sources and ordered observation_start/end |
| Benefit measured | All observing inputs plus actual_value, measured_by/at and measurement_evidence; measurement cannot precede the observation end date |

UAT requirement scope must equal the selected PBIs' satisfies scope. Selected cases must stay within it and collectively cover it. Feature/migration items must pin the UAT baseline. Latest test results are evaluated against the immutable approved requirement versions: unrelated/newer draft versions do not replace them, an older selected pass cannot hide a newer failure on the same version, and simulations cannot establish acceptance. The review must follow selected test executions; the go decision must follow UAT acceptance. Done, UAT accepted, ready and released remain separate recorded states.

This first UAT contract supports requirement-linked delivery on one baseline. Pure research/enabler releases without requirement-based acceptance and releases spanning multiple baselines need a separately designed contract; do not manufacture requirements to satisfy this one. Teams choose appropriate business cases and defect dispositions. The tool does not certify their semantic completeness, deploy software, identify a human signer or prove that an event happened.

Benefit values are displayed as supplied; the tool does not infer causality, target attainment or missing costs. Numeric zero is distinct from unknown. Currency conversion, cost aggregation, rate inference and financial authorization are outside this increment.

## Agent access, validation and compatibility

The Agent can call pm with command delivery and an empty args list, or a user can run pm-agent --vault PATH pm delivery. The result includes recorded state, current structural problems and next-gate information. It is read-only, needs no approval, and creates no output artifact. An available report can contain problems; inspect its problem and per-record problem fields. The same checks run during form preview/save and pm validate, including records edited in Obsidian. Invalid delivery-v1 releases cannot satisfy coverage.

The Agent's write_draft capability remains draft-only and cannot invoke the human editor's save path. A structurally complete next gate is not an actual decision; request the missing event or evidence instead of setting an outcome automatically.

Existing Change and Release records without workflow_schema retain legacy validation/coverage behavior. The Delivery view explicitly labels them legacy and excludes them from editing and delivery-v1 readiness claims. It does not silently migrate history. Create a delivery-v1 draft linked through previous_records, supply actual evidence, and review it. UAT and Benefit records require the new schema; unknown schema values fail validation.

The view rechecks current inputs when loaded or refreshed, including [automatic refresh](Automatic-Refresh.md) after local knowledge or baseline changes. Later failed tests, rescinded baselines or changed links can therefore flag an old recorded outcome; its original state and bytes are retained. These findings prompt review and do not automatically revoke a historical decision. Signed/tamper-proof audit is not implemented.
