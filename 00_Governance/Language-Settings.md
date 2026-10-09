---
type: "Reference"
language: "en"
title: "Project document language"
---

# Project document language

Edit `document_language` in [Settings](Settings.md), using Obsidian Properties or the local tool.

| Value | New project content |
|---|---|
| en or en-US | English; en is the starter default |
| zh-TW | Traditional Chinese |
| Another language tag | Agent-authored prose in that language; generated reports require a reviewed locale catalog |

Framework documentation and its filenames are English. AGENTS, Skills, Playbooks, agent work packets, Runs and operational reports always use English. Project charters, domain definitions, BRDs, PRDs, requirements, PBIs, decisions, tests and forecasts use `document_language`.

Use stable IDs for project record filenames, or names written in the document's language. Keep machine keys, enums, parser-consumed table headers, paths and code identifiers unchanged. Imported originals and immutable baselines retain their original language and bytes.

Changing Settings affects subsequent output; it does not translate existing records. For an existing project, specify the documents to migrate. Translate approved content into new drafts/versions and preserve the prior baseline. When starting a non-English project, have the agent localize the editable charter, context map, source index, mappings and planning input notes during setup.

The tools include `en` and `zh-TW` renderers. For another language, translate every value in `tools/locales/en.json` into `tools/locales/<tag>.json`, preserving keys and formatting placeholders, then review terminology and run validation. An absent or incomplete renderer stops output instead of silently using English.

See the authoritative [Language policy](../90_Agent/Language-Policy.md) and [OKF profile](../90_Agent/OKF-Profile.md). Metadata checks support review but cannot establish that the prose is a correct translation.
