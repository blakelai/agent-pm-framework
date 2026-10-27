---
type: "Reference"
title: "Project backup and restore"
language: "en"
---

# Project backup and restore

Use the operator CLI to save and recover the working Obsidian Vault. Recovery is local Rust code and needs no model, MCP server, Python, Git or original provider configuration. These commands are separate from the 19 Agent PM operations; they are not model tools or TUI slash commands.

## Create and verify

Close other PM Agent processes for this Vault and pause edits in Obsidian or other editors. Choose an existing backup directory outside the Vault and a new filename:

```sh
pm-agent --vault /projects/example backup create --output /backups/example.pmbak
pm-agent backup check /backups/example.pmbak
```

Add `--include-sessions` to `backup create` to include the private SQLite runtime. The database must already exist; otherwise creation fails explicitly. It contains conversations, tool arguments/results, unresolved operations and a rebuildable document index. SQLite's online backup API captures committed WAL content consistently; copying a live database file alone is insufficient.

Creation obtains the normal workspace writer lock, inventories the supported scope, hashes each file and rechecks project bytes and inventory before publishing. A private temporary archive is published without overwriting any existing destination. An external editor does not participate in the lock: these checks reduce conflicts but do not create a transaction across all project files. Pause external editing for a coherent snapshot.

`backup check` reads archive structure, limits, the manifest and every included file. It does not create runtime state or modify the archive. The JSON report contains the archive path, file count, total uncompressed bytes, format and session inclusion. A checksum proves correspondence with the supplied manifest, not authorship, business correctness or authenticity. SQLite schema/integrity checks also run when restoring sessions.

## Included scope

The versioned `project-knowledge-v1` scope is explicit:

| Included | Preservation |
|---|---|
| 00_Governance through 09_Operations, 80_Templates and 90_Agent | All files recursively, including source originals/extractions, captured repository/OpenWiki snapshots, immutable baselines, review history and operational Runs |
| .agents | Native Skills and their stored dependencies, unchanged |
| Root .md and .canvas files | Navigation, instructions and Obsidian canvas bytes |
| agent/*.md, tools/README.md and tools/locales/** | Application guides and project report catalogs |
| .pm-agent/runtime.sqlite3, only with --include-sessions | Consistent SQLite snapshot; pending/unknown outcomes remain unchanged |

Nested source files are preserved even when their names look like configuration or cache files. This is not secret detection or redaction. Project records and optional sessions may themselves contain sensitive content.

The root Git directory/history, application source/binaries/build output, development environments, arbitrary root attachments, Obsidian settings/plugins and private runtime configuration/probe observations are outside this scope. Model credentials and MCP/provider configuration in `.pm-agent/config.toml` are excluded. External registered repositories are not copied; only their captured Vault snapshots are included. Import arbitrary attachments into the source folders before relying on this backup.

Backups are private, unencrypted ZIP archives with a `manifest.json` and `vault/` entries. The manifest records the format, scope, application version, creation time, paths, byte counts and SHA-256 values. Keep archives in access-controlled storage appropriate for the project. On macOS/Linux, created archive/files use 0600 and restore directories use 0700. Original filesystem permissions, executable bits, extended attributes, empty directories and Git history are not preserved.

Limits are 20,000 files including optional runtime, 50 MiB per file, 1 GiB total uncompressed data, 8 MiB for the manifest and 16 MiB for the ZIP directory. Oversized inputs fail rather than silently producing a partial backup. Symlinks and special files in the included scope are rejected. Archive paths must be portable UTF-8 paths without traversal, control characters, backslashes or colons; duplicate/case-colliding names are rejected. The format does not accept ZIP64, multipart archives or ZIP comments.

## Restore independently

Use a trusted backup and an existing parent directory outside all project Vaults:

```sh
pm-agent backup restore /backups/example.pmbak --parent /projects
pm-agent backup restore /backups/private.pmbak --parent /projects --include-sessions
```

The command does not require `--vault` or a provider configuration. It creates a unique private child of the parent and returns that path only after file verification and, when requested, runtime database checks. It never overlays an existing Vault. Ordinary failures remove the staging directory. A hard process termination can leave a private staging directory; discard it and repeat recovery rather than treating it as a completed restore.

Session restoration is opt-in even when the archive contains sessions. Every archived file is hash-checked regardless of this choice. A request to restore sessions from a project-only archive fails. Pending tool ledger entries and request observations remain historical: restoration never retries, resolves or replays them.

After success:

1. Open the reported path in Obsidian. Use a separately installed/built PM Agent binary; this archive does not distribute the application.
2. Run `pm-agent --vault RESTORED_PATH pm validate` and inspect the findings. Integrity does not imply complete requirements or release readiness.
3. Reconfigure the local provider/MCP settings and credential environment through the existing [setup procedure](README.md). Review recorded external repository paths and custom Skill dependencies before using them on another computer.
4. Inspect restored sessions before resuming. Existing [reconciliation rules](README.md#clarification-recovery-and-persistence), Skill fingerprint checks and provider-profile rules still apply. No external operation has been authorized by the restore.
5. Initialize Git separately if sharing the recovered project is required.

## Compatibility boundary

This release reads `pm-vault-backup-v1` with `project-knowledge-v1`. Unknown formats/scopes and unsupported runtime database objects fail with an explicit migration message. The manifest retains the writing application version, but there is no historical database migration engine or promise of arbitrary future-version compatibility.

An [OKF export](../90_Agent/OKF-Profile.md) remains a knowledge distribution with transformed links and typed Skill representations. Recovery preserves working Vault bytes and native Skill adapters. Neither artifact replaces off-device storage or an exercised recovery procedure.

