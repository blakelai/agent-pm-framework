---
name: "pm-workflow"
description: "Start or continue cross-stage project management in an Agent PM Vault; select the intake, analysis, requirement, planning or review capability needed next."
---

# Project workflow entry point

Resolve the Vault root three levels above this file. First read [Settings](../../../00_Governance/Settings.md), [Language Policy](../../../90_Agent/Language-Policy.md), [AGENTS](../../../AGENTS.md) and [Project](../../../00_Governance/Project.md). Select the affected IDs and scope from the user's task; inspect the latest relevant Run and necessary upstream/downstream documents.

| Task | Capability |
|---|---|
| Source intake and review | [pm-intake](../pm-intake/SKILL.md) |
| System/integration analysis | [pm-system-analysis](../pm-system-analysis/SKILL.md) |
| Ubiquitous Language | [pm-language](../pm-language/SKILL.md) |
| Business requirements | [pm-brd](../pm-brd/SKILL.md) |
| Product requirements | [pm-prd](../pm-prd/SKILL.md) |
| Backlog and acceptance | [pm-backlog](../pm-backlog/SKILL.md) |
| Effort and delivery forecast | [pm-estimate](../pm-estimate/SKILL.md) |
| Change and delivery review | [pm-review](../pm-review/SKILL.md) |

Load only the capabilities and references needed for this task. Complete independent work while making missing inputs explicit. When inputs suffice, produce and check the artifacts rather than stopping with a prepared packet.

A single agent can switch capabilities. This Skill does not itself start subagents, background scheduling or external services. Additional delegation and external actions remain within the user's scope.

Complete when the task has reviewable artifacts, an English Run with effective language settings, actual check results and explicit remaining decisions. Consult [Workflow](../../../00_Governance/Workflow.md) for the lifecycle.
