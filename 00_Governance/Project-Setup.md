---
type: "Reference"
language: "en"
title: "Project setup"
---

# Project setup

1. Create a dedicated workspace from this repository. Retain the hidden `.agents` directory. The starter contains no demonstration data to remove.
2. Set `document_language` in [Settings](Settings.md). Use [Language settings](Language-Settings.md) when changing language. Write or translate editable project inputs in that language; keep framework procedures and agent artifacts English.
3. Complete [Project](Project.md): stable ID, business problem, measurable outcomes, scope, owners, decision rights and constraints. Change its `workflow_status` from `unconfigured` to `draft` when these initial inputs are recorded. Track unresolved decisions explicitly.
4. Register each repository with its local path, Wiki location and code scope using `repo-add`; capture a version with `sync`. Repository paths are local to the workspace and must be reviewed after moving it. See [OpenWiki integration](../03_Systems/OpenWiki-Integration.md).
5. Import materials and review extraction fidelity, diagrams and contradictory claims. Record evidence with locators. Create each bounded context from [Context](../80_Templates/Context.md) under `02_Domain/<context>/CONTEXT.md`, then update the context map and source index.
6. Develop a BRD, PRD, atomic requirements and a small set of verifiable PBIs using `80_Templates`. Assign real owners; keep unknown estimates and unplanned Sprints empty. Preserve links to source evidence and glossary definitions.
7. Set `as_of` in [Planning](../06_Delivery/Planning.md), add actual periods to [Iterations](../06_Delivery/Iterations.md), and record remaining availability in [People](../06_Delivery/People.md). Include each person's other project allocations when checking total FTE.
8. Run `validate` and `coverage`; run `plan` once planning inputs exist. Review missing evidence, estimates and dependency constraints with the accountable people. A successful tool run is not approval.
9. Capture candidate baselines and record actual decisions using [Baselines and changes](Baselines-and-Changes.md). Record team readiness, verification and release outcomes only after they occur.

Generated reports appear in `90_Agent/Reports` and `06_Delivery/Plan-Report.md`; agent work records appear in `90_Agent/Runs`. Keep these and real source data in the project's own repository. Framework contributions should contain reusable changes and tests only.
