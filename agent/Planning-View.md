---
type: "Reference"
title: "Interactive scheduling and capacity views"
language: "en"
---

# Interactive scheduling and capacity views

Press F5 in Browse or click Planning to open the planning view. It is a local project view and needs no model connection. Finish an active operation first. Esc returns to the document browser; the unfinished chat draft remains intact.

The view reads the current project records and calculates a snapshot using the same native engine as /pm plan. It shows the supplied forecast as_of and the actual load time. Opening or refreshing the view does not save records or replace a historical Plan-Report. Press R after external edits; automatic file watching is a separate milestone.

## Views and navigation

| Control | Behavior |
|---|---|
| 1 / 2 / 3, or click the top labels | Schedule / Capacity / Signals |
| Left / Right, or select an iteration | All PBIs, unscheduled work or a recorded iteration |
| Tab | Change focus between iterations, PBIs and details |
| Up / Down | Select a row or scroll the focused details |
| PgUp / PgDn, wheel, right scrollbar | Read longer details and allocation tables |
| Enter in the PBI list | Return to the document browser and read the selected PBI |
| M, right-click a PBI, or Move | Choose a proposed iteration assignment |
| E / P / I / A, or footer buttons | Edit selected PBI / People / Iterations / forecast date |
| R / Esc | Refresh from files / return to Browse |

Schedule lists actual project PBIs, including unknown estimates and historical completed/cancelled records. Detail shows supplied owner, dependencies, readiness window, required date, execution baseline and source content. Original and remaining effort come from the forecast; unavailable results remain explicitly unavailable. Templates are excluded.

Capacity shows role demand, available capacity and balance in remaining person-days. Its bars compare demand with capacity; numerical balances and OVER identify excess demand. These values do not assign individual PBIs to people or turn effort into calendar duration. Supplied allocation rows include all projects so that shared-person allocations remain visible; only this project's rows contribute to its calculated capacity. No capacity row means unavailable, not zero.

Signals are project-wide, regardless of the iteration selection. They include the existing dependency, blocked-work, readiness-window and deadline checks, plus arithmetic capacity excess and unknown effort. They do not produce a risk score, probability of delivery or project readiness approval. Use project validation for readiness, baseline and acceptance checks.

## Review a scheduling proposal

1. Select a PBI and press M, or right-click its row. Choose a recorded iteration or Unscheduled. This changes only an in-memory proposal.
2. Inspect the editor preview: destination, changed properties, planning notes, and before/after capacity, demand and balance for affected roles. The time basis is displayed explicitly. If prior inputs could not produce a forecast, the comparison says so instead of manufacturing a baseline.
3. Press Ctrl+S to save the reviewed proposal. Esc returns to editing; Ctrl+D discards with confirmation. Invalid inputs remain in the form for correction. A scheduling selection does not itself save anything.
4. After a successful save, the planning view reloads the actual files. The English edit record includes the reviewed changes and planning impact. An existing Plan-Report remains historical until /pm plan is run.

Moving work uses the same [structured editor](Editing.md) checks as manual PBI editing. Unknown scheduled estimates, invalid iteration references and missing readiness/baseline/acceptance inputs must be resolved. Forecast warnings such as excess demand or a dependency conflict are review inputs, not an automatic business decision. Empty setup inputs can leave a forecast unavailable; a reviewed record change does not establish an available delivery plan.

The selected PBI hash must still match the loaded view. The editor then checks project inputs again before saving. External changes require refresh, reconciliation or a new preview. These are optimistic checks, not a transaction with other editors. Completed/cancelled PBIs cannot be moved through the scheduling picker, and approved records retain their existing protection.

## Scope

This view supports one reviewed PBI move at a time and typed editing of existing planning inputs. It does not optimize assignments, choose dates, split work, infer holidays, infer consumed effort or establish commitments. [Delivery workflows](Delivery-Workflows.md) provide separate Change/UAT/Release/Benefit screens. Bulk schedule scenarios, a calendar-duration Gantt model and broader live-project evaluation remain separate work.

See [planning rules](../06_Delivery/Planning.md), [interaction controls](Interaction.md) and [implementation milestones](Implementation.md).
