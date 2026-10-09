---
id: "PLAN-CONFIG"
type: "planning_config"
as_of: null
language: "en"
title: "Rolling forecast rules"
---

# Rolling forecast rules

People is the only capacity input. Set as_of to an ISO date (YYYY-MM-DD), then provide actual iterations and remaining allocations before generating a forecast. Empty starter inputs are unconfigured, not a zero-effort delivery plan.

1. Update as_of and remaining Workdays/Absence after that date. Do not infer holidays or consumed hours.
2. Proposed/ready work uses original O/M/P. In_progress/blocked work requires a separate Remaining O/M/P table. Done/cancelled work consumes no future capacity.
3. Unscheduled PBIs use sprint:null and may have unknown estimates. Scheduled missing estimates block reporting. Do not derive remaining effort as original estimate times percent incomplete.
4. A person's FTE across all roles/projects in one Sprint must total at most 1. Use the same remaining workdays for that person/period; include only this project's rows in its capacity.
5. Capacity = (FTE * Workdays - Absence) * Focus * (1 - Reserve). Absence is person-days allocated to this assignment. Avoid counting an allowance twice.
6. ready_after is external readiness; due_date is required delivery. Unknown dates stay open. The tool detects windows/dependencies but does not optimize a critical path.
7. Expected three-point effort = (O + 4*M + P)/6. This is not P80 delivery probability. Probabilistic forecasting requires comparable historical delivery data and explicit assumptions.

Preserve parser-consumed table headers in English. The generated Plan-Report follows document_language.
