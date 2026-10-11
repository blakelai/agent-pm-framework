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

Load only the capabilities and references needed for this task. For unclear, missing or conflicting inputs, apply [Evidence and Clarification Policy](../../../90_Agent/Evidence-and-Clarification-Policy.md): ask the user for clarification or relevant material and keep dependent work pending. Unsupported inference is prohibited. Continue only supported independent work. When inputs suffice, produce and check the artifacts rather than stopping with a prepared packet.

A single agent can switch capabilities. This Skill does not itself start subagents, background scheduling or external services. Additional delegation and external actions remain within the user's scope.

Complete when the requested artifacts are supported by sufficient evidence and the English Run records effective language settings, actual check results and remaining decisions. If a requested outcome still depends on an unanswered question, record the Run as blocked and identify completed independent work. Consult [Workflow](../../../00_Governance/Workflow.md) for the lifecycle.
