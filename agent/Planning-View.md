---
type: "Reference"
title: "Interactive scheduling and capacity views"
language: "en"
---

# Interactive scheduling and capacity views

Press F5 in Browse or click Planning to open the planning view. It is a local project view and needs no model connection. Finish an active operation first. Esc returns to the document browser; the unfinished chat draft remains intact.

The view reads the current project records and calculates a snapshot using the same native engine as /pm plan. It shows the supplied forecast as_of and the actual load time. Opening or refreshing the view does not save records or replace a historical Plan-Report. [Automatic refresh](Automatic-Refresh.md) detects external knowledge edits; R requests an immediate refresh. The selected record and reading position are retained when possible. A destination picker closes after changes so its choices can be reviewed again.

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
| S, or Scenarios | Compare saved simulations and review one for application |
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

## Compare planning scenarios

Prepare a JSON file with two through eight alternatives. Each alternative contains one through twenty sprint moves for distinct active PBIs. Use a recorded sprint number or an explicit `null` to unschedule an item.

```json
{
  "scenarios": [
    {"id":"Swap","moves":[{"pbi":"PBI-A","sprint":2},{"pbi":"PBI-B","sprint":1}]},
    {"id":"Defer","moves":[{"pbi":"PBI-B","sprint":null}]}
  ]
}
```

```sh
pm-agent --vault . scenario compare alternatives.json --id NOV-PLAN
pm-agent --vault . scenario show NOV-PLAN
pm-agent --vault . scenario review NOV-PLAN Swap
pm-agent --vault . scenario apply NOV-PLAN Swap --expect-review REVIEW_SHA256
```

All forecasts in a comparison use the same captured project, People, Iterations, Planning, Settings, locale and PBI versions. The report records each input path and SHA-256 and labels results as simulations. Each alternative includes role capacity and demand, PBI effort and assignments, and forecast signals such as later or unscheduled dependencies. A blocked alternative shows its diagnostic beside any valid alternatives. Saved comparisons in `.pm-agent/planning-scenarios/` retain the original results and require a new ID for a fresh comparison.

Create the comparison with the CLI, then open **F5 Planning → S Scenarios**. Select a comparison with Enter and use Up/Down to switch alternatives. The detail shows the selected simulation against the recorded schedule. Enter opens an application review. Scroll with PgUp/PgDn and press Ctrl+S to apply that exact review. The CLI uses the `review_sha256` printed by `scenario review` for the same confirmation.

The application review checks each PBI with the structured editor, verifies the combined forecast and rechecks the pinned planning and validation inputs before writing. If planning inputs changed, create a new comparison. If only validation inputs changed, inspect a fresh review. Applying the selected alternative updates sprint assignments and writes an English edit audit. The Plan-Report remains historical until `pm plan` is run.

An interrupted batch keeps a local recovery journal. Run `scenario recover` to finalize an application whose audit was committed or restore the previous assignments. A conflicting external edit leaves recovery pending for reconciliation. Forecast recalculation, scenario application and Vault backup require a consistent schedule.

## Scope

Planning supports individual reviewed moves, batch sprint scenarios and typed editing of existing planning inputs. Scenario capacity, dependency relationships, estimates and forecast dates come from the pinned records. Delivery workflows handle Change, UAT, Release and Benefit separately. Calendar-duration modelling and broader live-project evaluation remain future work. [Delivery workflows](Delivery-Workflows.md)

See [planning rules](../06_Delivery/Planning.md), [interaction controls](Interaction.md) and [implementation milestones](Implementation.md).
