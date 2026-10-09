---
id: "PROJECT-SETTINGS"
type: "project_settings"
document_language: "en"
agent_language: "en"
language: "en"
title: "Project language settings"
---

# Project language settings

Edit `document_language` in Obsidian Properties to select the language of newly authored project content. Examples: `zh-TW` (Traditional Chinese, Taiwan), `en` (English), `ja-JP` (Japanese, Japan). Use an explicit regional or script variant when it matters. The clean framework starts with English project inputs; select your project's language during setup.

`agent_language` is fixed to `en`. The complete scope, preservation rules, and language-change workflow are defined in [[90_Agent/Language-Policy]]. This configuration is the single source of truth; the language of a chat message or source file does not change it.

Changing this property does not translate existing files, rename identifiers, or alter approved baselines. Ask the agent to migrate a named set of documents when needed. Built-in project-report catalogs cover `zh-TW` and `en` (including English regional variants). For another language, the agent must add and review a complete catalog before running project report generation. Agent-authored BRD, PRD and PBI content can use the configured language directly.

User instructions: [[00_Governance/Language-Settings]]
