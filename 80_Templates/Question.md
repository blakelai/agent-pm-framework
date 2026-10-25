---
type: "question"
language: "en"
title: "Open question"
id: "QUESTION-NEW"
workflow_status: "open"
clarification_schema: "clarification-v1"
blocking_scope: "project"
affected_ids: []
affected_paths: []
owner: null
due_date: null
---

# Open question

## Question and decision needed

State the exact ambiguity, missing information or contradiction. Write the question that must be presented to the user.

## Evidence and known constraints

Link inspected sources with versions and locators. Distinguish source statements from supported deductions and identify the remaining gap.

## Requested material or answer

Specify the document, page, diagram, contract, measurement, capacity input or stakeholder answer needed to resolve the gap. Record when and how the request was surfaced to the user.

## Options and impact of waiting

Set blocking_scope to records only when evidence establishes a bounded scope; list existing affected_ids and/or exact managed affected_paths (including planned files). Otherwise keep project scope and empty target lists. Link affected document IDs and identify dependent conclusions or actions on hold. List any work that can proceed independently. Include only evidence-based options; leave unknown values pending.

## Owner follow-up and resolution

Keep owner and due_date null until provided. Record the actual response, its source/date/scope, unresolved parts and the affected drafts to update. Use `/answer QUESTION-ID TEXT` in the application to capture the actual response. A manual open question without a session is associated with that session by this explicit action. Resolution requires the captured response, current question/scope fingerprint and an explanation; see [scoped clarification](../agent/Clarification.md). Apply the [Evidence and clarification policy](../90_Agent/Evidence-and-Clarification-Policy.md).
