---
type: "Reference"
title: "Structured project editing"
language: "en"
---

# Structured project editing

Browse mode can edit existing People, Iterations and Planning notes, PBIs under 05_Backlog, and risks or ordinary draft decisions under 07_Decisions. Press E or choose Edit project fields from the document context menu. N creates a new PBI, risk or decision with a user-supplied stable ID. No model request is needed. Finish or cancel an active operation before opening an editor.

## Edit, review and save

The field list and input pane support keyboard and mouse selection. Enter or Tab opens a field; Tab applies it back to the form. Enter also applies single-line fields. Lists use one entry per line; document content supports multiline Markdown. Left/right selects enum values. Add row and Remove row operate on the in-memory table; empty numeric cells remain unknown rather than zero.

Ctrl+S validates the proposal and displays property changes and the changed body lines. Nothing has been written at this point. Inspect the destination, differences and validation notes, then press Ctrl+S again to save. Clicking the corresponding footer buttons performs the same actions. Esc returns from preview to the form; in an active field, Esc reverts that field's uncommitted input. Ctrl+D closes the editor and asks before discarding unsaved changes. Other form edits and the unfinished chat draft remain intact.

Planning-related edits also show affected role capacity, demand and balance before and after the proposal when calculable. Missing prior or proposed forecasts remain explicitly unavailable. A move from the [planning view](Planning-View.md) opens this preview directly; Ctrl+S still performs a separate save. The audit records the same reviewed planning impact.

Existing IDs, document types, language metadata, unexposed properties and surrounding prose are preserved. Editable tables keep their parser-consumed English headers. Saving normalizes frontmatter to the framework's existing flat Properties format. New documents use the configured project language; their title, body, business inputs and stable ID come from the user. The editor does not translate or invent missing content.

## Supported inputs

| Record | Editable inputs |
|---|---|
| People | Person, role, Sprint, project, FTE, remaining workdays, absence, focus and reserve; add/remove allocation rows |
| Iterations | Integer Sprint, start/end ISO dates and goal; add/remove periods |
| Planning | Explicit forecast as_of date |
| PBI | Owner, work type, delivery state, Sprint, context and traceability links, dependencies, readiness dates, original/remaining estimates, readiness/acceptance/DoD evidence, AC mapping and document content |
| Risk | Owner, due date, open/closed state, evidence and content |
| Draft decision | Owner, context/evidence, state, actual decision actor/date, decision evidence and content |

Rows are checked for invalid dates/numbers, duplicate keys, overlapping periods, inconsistent calendars and over-allocation. People and unfinished scheduled PBIs must refer to recorded Sprints, even during setup. The existing forecast engine evaluates the candidate without writing a report. During setup, missing periods, forecast date or current-project allocations produce an explicit unavailable-forecast note; the editor does not manufacture those inputs. Invalid supplied values still fail. A saved edit does not refresh an existing Plan-Report: run /pm plan when the planning inputs are ready. Preview checks the edited record and its planning inputs; use /pm validate for a full workspace quality report.

## Workflow and evidence boundaries

PBI state selection uses the existing delivery_status values and the same record validator as the quality command. Ready, in-progress, blocked and done states require readiness records. Feature/migration execution requires an approved baseline containing the relevant PRD, requirements and glossaries. Done records require acceptance/DoD evidence; applicable work also needs AC mappings and actual passing results for its pinned requirements. A simulation cannot satisfy that check. Scheduled work requires estimates, with separate remaining effort for active work. An existing execution baseline cannot be replaced through this editor; follow the Change procedure.

Once a PBI has left proposed state, its work type cannot be changed here. Refine a proposed item through the Change procedure so that readiness and baseline requirements cannot be bypassed by changing its classification.

Ordinary decisions can record accepted, rejected or rescinded outcomes only with supplied actor, date and resolvable decision-evidence links. Previously recorded decisions are read-only here: create a new linked decision to preserve history. Baseline-approval decisions and approved/retired documents use their separate procedures. Closing a risk requires evidence links. These checks establish structure and references, not whether a human decision or event actually occurred; the user remains responsible for the supplied facts.

Sources, snapshots, immutable baselines, templates, Skills and general governance documents are outside this editing surface. Agent write_draft keeps its separate, existing draft-only restrictions; the model cannot call the human editor's save operation.

## Conflicts and audit

The editor compares the original document before preview and again before save. It also compares the project knowledge hashes and rechecks the candidate when saving a reviewed proposal. A changed document, changed evidence note or different validation result requires reconciliation or a new preview; the current draft is retained. This is optimistic checking, not an atomic transaction with external editors such as Obsidian. File replacement itself is atomic.

A successful save creates an English record under 90_Agent/Runs with before/after hashes, reviewed changes and validation notes. It records local TUI initiation without inventing a human identity. If the document saves but its audit write fails, the UI explicitly reports that partial result and does not present the document save as unexecuted.

See [interaction controls](Interaction.md), [project schema](../00_Governance/Schema.md) and [planning rules](../06_Delivery/Planning.md). Richer delivery-specific screens and backup/migration tooling remain separate milestones.
