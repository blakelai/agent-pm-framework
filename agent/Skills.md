---
type: "Reference"
title: "Custom project Skills"
language: "en"
---

# Custom project Skills

PM Agent discovers project-local Skills at `.agents/skills/<name>/SKILL.md`. Discovery does not activate instructions. The workflow stage always selects its existing PM Skill; additional Skills require an explicit user selection.

## Discover and select

Use `pm-agent --vault <vault> skills` for a JSON catalog without a model request. The catalog reports malformed Skills, missing instruction references, missing required Skills and dependency cycles. Invalid Skills do not disable unrelated valid Skills or document browsing.

In the TUI, `/skills` opens a keyboard/mouse picker. Choosing a Skill inserts a leading `$skill-name` into the draft; nothing is sent until Enter. You can also type `$risk-review $pm-review Review the supplied evidence.` directly. Only leading tokens in an actual user message activate Skills. Mentions inside quoted source data, tool results or ordinary message prose do not activate anything.

The CLI supports repeatable `chat --skill risk-review --skill pm-review "Review the supplied evidence."`. Additional selections persist in the session. Use `/skills clear` to return to the stage default, or `/new` for a new session. Selections cannot change while a tool is pending. There is no automatic selection from a description, global home-directory discovery, installation from remote repositories or script execution.

## File contract

The folder and name must match: 1–64 lowercase ASCII letters, digits or single hyphens; no leading/trailing or repeated hyphens. A nonempty description and instruction body are required. Skill prose must be English. Optional `language`, when present, must be `en`; structural validation does not certify the actual prose language.

This is a format example, not an installed project Skill:

```yaml
---
name: risk-review
description: Review documented project risks against supplied evidence.
language: en
pm_agent:
  version: "1.0"
  requires:
    - pm-review
  references:
    - references/checklist.md
---
```

Below the frontmatter, write English instructions with explicit inputs, outputs, evidence requirements and missing-information handling. Keep reusable workflow methods in a referenced Playbook. Project outputs follow `00_Governance/Settings.md`; source quotations and identifiers retain their original language.

The optional `pm_agent` block is this application's extension:

| Field | Meaning |
|---|---|
| version | Human-declared string; omitted versions remain undeclared |
| requires | Other project Skill names, resolved before this Skill |
| references | Local Markdown/text instruction files, relative to this Skill folder |

References are loaded only when the Skill is selected. Paths may address another location inside the allowed Vault knowledge scope; escaping paths, symlinks, non-text references and unreadable files are rejected. Required Skills are deduplicated and cycles fail. Body links are navigation; they are not automatically promoted into instruction references. Declare required instruction files explicitly.

Native Skill adapters retain their runtime metadata format. Their knowledge representations are included by [OKF export](../90_Agent/OKF-Profile.md).

## Evidence and execution limits

Each runtime invocation records the selected stage Skill, explicit Skills, declared dependencies, versions and SHA256 hashes of the exact loaded instruction files. The stage Playbook is included in that record. The SQLite session retains activation history, and the English Run lists those activations. These are content fingerprints, not fabricated Git versions or semantic approval.

A selected Skill change is picked up on the next turn and recorded as another activation. If a tool is already pending, instruction changes block continuation until the recorded files are restored. Missing or invalid explicitly requested Skills fail before sending a model request.

Skills define methods and prompts; tools remain the existing native PM tools and configured MCP allowlists. Adding a Skill does not grant filesystem scope, shell execution, new tools or approval bypass. Bundled scripts are not executed. Governance, source provenance, mandatory clarification and baseline protections remain in effect.

See [terminal operation](README.md) and [interaction contract](Interaction.md).
