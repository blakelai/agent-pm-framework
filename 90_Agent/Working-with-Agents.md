---
type: "Reference"
language: "en"
title: "Working with the agent"
---

# Working with the agent

Use this Vault as the agent's working directory. If a Skill is not listed by the host, ask the agent to read its `.agents/skills/.../SKILL.md` explicitly. The prompts below describe task types. Substitute actual record IDs from your project; the starter contains none of these records. Generated project prose follows Settings.

- Use pm-workflow. Read Settings, HOME and Project; inspect current requirements, risks and open decisions, then complete independent analysis and save a Run.
- Use pm-intake to import the supplied materials. Separate original statements, assumptions and solution proposals, and record visual-review gaps.
- Use pm-prd to turn BRD-001 into a PRD and atomic requirements in document_language. Apply system evidence and glossary terms; keep unconfirmed thresholds explicit. Run validate and coverage.
- Use pm-backlog to split REQ-001/002 into verifiable PBIs; create timeboxed Spikes for unknowns and leave unplanned work unscheduled.
- Use pm-estimate with current People and remaining estimates. Identify overload, external waits and conditional options.
- Use pm-review to assess a new REQ-001 version without replacing old execution baselines.
- Change document_language in Settings, then migrate only the document IDs specified by the user. Preserve source quotations, identifiers and historical baselines.

See the [tool manual](../tools/README.md) and [project setup guide](../00_Governance/Project-Setup.md).
