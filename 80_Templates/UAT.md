---
type: "uat"
id: "UAT-NEW"
title: "User acceptance review"
language: "en"
workflow_schema: "delivery-v1"
workflow_status: "draft"
owner: null
previous_records: []
evidence: []
baseline_id: null
items: []
requirements: []
cases: []
test_runs: []
reviewed_by: null
reviewed_at: null
decision_evidence: []
---

# User acceptance review

## Business scenarios and acceptance measures

Record the supplied business scope and link selected cases. Do not infer acceptance from implementation completion.

## Baseline, items and test execution

Link actual completed items, pinned requirements, cases and version-matched test runs. Keep simulations separate.

## Remaining defects and reconciliation

Record the business owner's treatment of defects, integration exceptions and migration reconciliation where applicable. Request missing evidence.

## Actual acceptance decision

Supply reviewed_by, reviewed_at with timezone, and decision_evidence only for an event that occurred. Accepted or rejected records are historical; create a linked new record for a correction.
