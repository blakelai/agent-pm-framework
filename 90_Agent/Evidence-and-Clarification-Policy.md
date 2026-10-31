---
type: "Reference"
language: "en"
title: "Evidence and clarification policy"
---

# Evidence and clarification policy

This policy applies to every project-management stage, including direct Skill or Playbook use. When task information is unclear, missing, contradictory, unreadable or unsupported, the agent must ask the user for clarification or request the relevant material. Unsupported inference is prohibited. Recording a gap or labeling a guess as an assumption does not satisfy this requirement.

## Resolve an information gap

1. Check the supplied material and authorized project sources for an explicit answer. Cite the applicable source and locator; preserve conflicting statements instead of choosing one without evidence. Reuse a recorded answer only when its scope and version apply to the current task.
2. For every unresolved gap, ask a specific question or request the exact missing material. State what is unknown, why it is needed and which output depends on it. Group related questions for readability. Ask for a responsible owner or response date when needed; leave those fields null until supplied. Contacting other people requires existing user authorization.
3. Record the question, requested material, sources already inspected and affected records using the [Question template](../80_Templates/Question.md). Keep unresolved values pending or null, using the configured project language. A saved question must also be surfaced to the user; silently creating a note is insufficient.
4. Pause conclusions and actions that depend on the missing answer. Continue only independent work with sufficient evidence. Silence, elapsed time, a template default, a typical industry value or model confidence does not resolve the gap. Keep the Run blocked if the requested outcome still depends on an unanswered question, and list any independent work completed.
5. On receipt of an answer or material, record its source, date, applicable scope and resolution. Check whether it actually resolves the question, update affected drafts and repeat relevant checks. Follow baseline/change rules for approved content. Resume dependent work only after the required information is available.

## Evidence and reasoning

- Project facts and source claims need traceable provenance, such as a user response, document page, slide, image region, repository revision and lines, or actual execution record. A reported claim is not automatically a verified fact.
- Evidence-based deductions and calculations may be used when their premises, method and limits are explicit. If a conclusion requires an unsupported premise, ask for that premise instead of filling it in.
- Record assumptions explicitly supplied by a stakeholder as that stakeholder's assumptions, with attribution and confirmation state. User-requested conditional scenarios remain scenarios; they do not establish actual requirements, capacity or commitments. An agent-created assumption cannot be used to bypass a missing answer.
- Estimates require supplied inputs or an explicit evidence-based method. Request missing scope, complexity, availability or historical data. Do not invent O/M/P values, FTE, owners, deadlines, acceptance thresholds, system capabilities, approvals or test results. Unknown effort is not zero.

## Completion

Before drafting, map each business assertion to a source passage or an explicit, supported deduction. Check inherited BRD/PRD claims against original evidence as well as their upstream document. Use these distinctions when reviewing:

- Missing measurements remain unknown; ask for a baseline and observation method. A universal validation rule does not establish an observed percentage or an agreed business KPI.
- Static code is a bounded implementation observation. Confirm the deployed process before stating current capabilities or claiming their absence.
- Excluded integrations define scope; unprovided dependency information stays unknown until the system inventory supports a completeness claim.
- Exception-handling sections identify questions when timeout, atomicity, retry or compensation behavior is unspecified. Templates identify topics to investigate; they do not supply requirements.

The Rust runtime applies an [automated source-to-claim review](../agent/Grounding.md) before Agent draft writes. Its findings trigger actual scoped questions. A supported model judgment does not replace the review obligations below.

Before marking work complete, confirm that every detected gap was asked about, each relied-upon answer has provenance, and no requested conclusion depends on an unresolved gap. List unresolved questions and their affected work in the English Run and in the response to the user. Placeholder templates and isolated test fixtures remain explicitly non-project data.

Local validation checks structure and selected evidence fields. Passing it does not prove that a claim is grounded or that an agent followed this policy; review the actual sources, questions and outputs.
