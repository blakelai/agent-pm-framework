---
type: "Reference"
title: "Diagnosing Chat startup failures"
language: "en"
---

# Diagnosing Chat startup failures

Chat loads project guidance and configured tools when starting work. An older or incomplete Vault can open in Browse while still lacking files needed by its selected Chat stage. A missing MCP executable can also prevent Chat from starting. The operating-system message `No such file or directory (os error 2)` alone does not identify which condition occurred.

## Read the diagnostic

Chat failures have a red, collapsed heading. Click the heading or press Ctrl+T to expand it. Expanded diagnostics preserve literal paths, punctuation and line breaks; they are not interpreted as Markdown links. The message retains the failed operation, available file/configuration/server context, the complete cause chain and relevant next steps. A missing-file example is:

```text
Failed: Cannot prepare Chat instructions
Stage: workflow
Vault: /path/to/project
Required guidance must be present and readable before contacting the model.

Caused by:
1. Cannot inspect project file metadata
Path: /path/to/project/90_Agent/Evidence-and-Clarification-Policy.md
2. No such file or directory (os error 2)

Next steps:
Check the exact path or executable shown above.
```

This is an illustrative path and an excerpt of the diagnostic, not an observation about a particular user's project. An MCP launch failure instead identifies the configured server and executable and preserves the operating-system cause. Startup errors distinguish process launch, MCP initialization and tool discovery. New contexts omit configured command arguments and environment values. Configuration parse failures report a safe category and line/column without printing raw TOML contents.

Failures recorded during execution remain in the private session database and reappear when the session is resumed. Failures before session input is accepted, such as MCP startup errors, appear in the current conversation but do not establish a saved run. Old sessions that saved only a generic message retain that message; missing historical paths are not reconstructed or invented. Shared Run notes continue to omit raw service errors.

## Check an older Vault offline

Run the installed binary against the actual project path. Replace the sample paths below with your own:

```sh
pm-agent --vault "/path/to/project" doctor
pm-agent --vault "/path/to/project" --config "/path/to/config.toml" doctor --stage prd
```

`doctor` prints the resolved Vault and configuration paths, then checks language settings, required governance documents, the selected stage's Skill and declared dependencies/references, and its Playbook when applicable. The default stage is `workflow`; use the same stage as the failing conversation. Independent failures are reported together and produce a nonzero exit status. Other explicitly selected custom Skills can be inspected with `pm-agent --vault "/path/to/project" skills`.

These checks do not create runtime state or acquire the writer lock, start MCP processes, make model requests or repair files. Provider configuration and API-key environment-variable presence are checked locally; key values are not printed. A pass does not establish MCP executable availability, live service compatibility or model behavior, nor does it validate every stage or a saved session's additional Skill selection.

## Use the reported context

| Diagnostic | Check next |
|---|---|
| Cannot prepare Chat instructions; path under the Vault | Confirm `--vault`. Compare the specific missing or unreadable file with the starter belonging to the installed framework version. Review customizations before restoring or updating that file. |
| Cannot load a stage Skill or declared reference | Inspect the named `.agents/skills/.../SKILL.md`, its declared dependencies and references. Use `skills` to see invalid entries. |
| Cannot start MCP server using command ... | Check `[mcp.NAME].command`, the executable search path in the environment launching PM Agent, and the script interpreter. The named program may exist while its interpreter does not. |
| MCP initialization or tool discovery failed | Check that server's startup and transport configuration. A successful process launch does not establish that it serves the MCP protocol. |
| Cannot read or parse provider configuration | Confirm `--config`; without it, the file is `.pm-agent/config.toml` under the selected Vault. Check the reported line/column and compare the schema with `config.example.toml`. |
| Cannot open/save session storage or publish an output file | Check the reported database/output path and directory permissions. Preserve the runtime database and use the [backup/recovery procedure](Backup-and-Restore.md) when needed. |
| Cannot read UTF-8 text | Check that an instruction file is text. Use the source extraction/review workflow for binary materials. |

Project configuration, custom guidance, approved baselines and session data are not interchangeable with a clean starter. No automatic overwrite or fallback guidance is applied. After addressing the reported cause, rerun `doctor` for the selected stage. Resume an accepted failed session with `/continue`; if startup rejected the input before saving it, submit it again after correcting the configuration.

For unresolved failures, provide the expanded diagnostic, stage, binary version, launch command with credential values omitted, and the offline `doctor` result. See [Interaction](Interaction.md) for controls and [Storage migrations](Storage-Migrations.md) for database-version errors.
