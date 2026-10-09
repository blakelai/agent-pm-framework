---
type: "Reference"
language: "en"
title: "Skills and Playbooks"
---

# Skills and Playbooks

The Vault contains one workflow entry point and eight capability Skills under `.agents/skills`. Keep automatic selection enabled; discovery depends on the host. The entire Vault is the distributable unit. This package does not install global Skills or a plugin.

Before writing, every Skill reads [Settings](../00_Governance/Settings.md) and [Language Policy](Language-Policy.md). Skills and Playbooks stay in English; project deliverables follow document_language.

| Skill | Canonical method |
|---|---|
| pm-workflow | [Workflow](../00_Governance/Workflow.md) |
| pm-intake | [Intake](Playbooks/01-Intake.md) |
| pm-system-analysis | [System analysis](Playbooks/02-System-Analysis.md) |
| pm-language | [Ubiquitous Language](Playbooks/03-Language.md) |
| pm-brd | [BRD](Playbooks/04-BRD.md) |
| pm-prd | [PRD](Playbooks/05-PRD.md) |
| pm-backlog | [Backlog](Playbooks/06-Backlog.md) |
| pm-estimate | [Forecasting](Playbooks/07-Estimate.md) |
| pm-review | [Review](Playbooks/08-Review.md) |

Edit the visible Playbooks in Obsidian; avoid duplicating methods inside Skills. See [usage examples](Working-with-Agents.md) and [evaluation method](Evals/Evaluation-Method.md).
