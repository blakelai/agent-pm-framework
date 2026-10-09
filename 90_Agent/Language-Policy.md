---
type: "Reference"
language: "en"
title: "Language policy"
---

# Language policy

## Resolve the language before writing

Read `00_Governance/Settings.md` on every task, including direct Skill or Playbook invocation. `document_language` is the single project output setting; `agent_language` is fixed to `en`. The language of the user's chat, a template, a repository or imported material is not a substitute for this setting. Keep conversational replies in the user's preferred language.

| Content | Language |
|---|---|
| Authored project content: charter, BRD, PRD, REQ, SYS, glossary definitions, PBI/AC, decisions, contracts, test cases, UAT and delivery plans | Configured document_language |
| AGENTS.md, root navigation index/log, everything under .agents, Playbooks, operational Runs, evaluations and reports under 90_Agent | English |
| Framework guides and navigation (README, HOME, References, project setup, language settings, source integration, tool manual), plus canonical execution references: Schema, Workflow, baseline/change procedure, discovery checklist, integration checklist, Planning rules, acceptance/release procedure, benefits/cost procedure, tool manual | English |
| 80_Templates | English source templates; translate headings and instructional prose when instantiating a project document |
| Source originals, extracted text, excerpts, repository/OpenWiki snapshots, immutable baselines and historical evidence | Preserve the original content and language |
| YAML keys, enum values, IDs, paths, anchors, API/code identifiers, parser-consumed table headers and numbers | Preserve the technical contract |

Framework documentation uses English filenames and English prose. Name new project records with stable IDs or words in the configured document language. When an authorized language migration renames a file, update incoming links and navigation together. Preserve runtime filenames, source originals and immutable baseline paths. Domain terms can appear verbatim as evidence or identifiers inside an English explanation. For cross-language glossary work, keep stable term IDs and context boundaries, record the target-language canonical term and original alias explicitly, and retain unconfirmed mappings as draft. Do not translate Avoid markers or code identifiers merely to make a language check pass.

All authored knowledge also follows [OKF Profile](OKF-Profile.md). Native Skill adapters retain host-compatible metadata; their knowledge representations are included in the OKF export.

## Author or update project content

1. Read the applicable glossary and sources, then draft in document_language. Translate source claims faithfully while keeping an exact original quotation and locator when needed. Label any explanatory translation.
2. Use English templates as structural input. Localize human headings and prose; retain property names, status values and parser-consumed table headers. Stamp authored notes with `language: <effective tag>` for traceable reviews.
3. Check wording, grammar, terminology and the configured language before finishing. Character heuristics cannot certify semantic language quality. Record reviewed scope in the English Run.

## Change a project's language

Editing Settings changes the default for subsequent work. It does not bulk-translate existing documents or modify source evidence. Inventory documents and their `language` metadata, report missing or different language metadata, and complete only the migration scope authorized by the user. Existing approved documents remain pinned; translate into a new draft/version using the baseline/change procedure. Never rewrite immutable snapshot bytes. IDs, links and test expectations must remain consistent; changes in business meaning require separate review.

## Deterministic tools

`pm.py settings` displays the effective policy; `settings --document-language TAG` updates only the project setting. Built-in project renderers are en and zh-TW; English regional variants share en. For any other tag, translate `tools/locales/en.json` into `tools/locales/TAG.json`, preserving every key and formatting placeholder. Read the result for natural language and glossary accuracy, then run validate and the affected generator. Missing or incomplete catalogs fail before project output is overwritten; there is no silent English fallback.

Project-generated wrappers and the delivery plan follow the catalog. Existing source excerpts, user-entered claims, goals and names retain their content; the agent must review or migrate authored prose explicitly. Diagnostics, command help, snapshots' technical metadata wrappers and all Agent operational reports remain English. A project renderer failure leaves any prior report historical; it is not a fresh forecast.

Language validation checks settings, catalog structure and authored-note metadata. It does not prove that prose actually matches the declared language. A new language catalog is usable only after an agent or human reviews its translation.
