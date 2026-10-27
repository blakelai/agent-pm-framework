---
type: "Reference"
title: "Project backup and restore"
language: "en"
---

# Project backup and restore

Use the operator CLI or the TUI Recovery panel to save and recover the working Obsidian Vault. Recovery is local Rust code and needs no model, MCP server, Python or Git. These operations are separate from the 19 Agent PM operations; they are not model tools. The standalone CLI also works when the original Vault and provider configuration are gone.

## TUI Recovery panel

In Browse, press F11, click Recovery in the top bar, or choose F11 Recovery from the document context menu. The panel is unavailable while an Agent or PM operation is running. Existing Chat input, session, model and usage remain unchanged.

| Control | Behavior |
|---|---|
| Alt+1 / Alt+2 / Alt+3 or click a tab | Select Backup / Check / Restore; paths are retained and session inclusion resets when changing operation |
| Tab / Shift+Tab or click a field | Select the archive path, restore parent, optional sessions checkbox or action |
| Space / Enter on the checkbox | Explicitly include saved sessions for this operation |
| Ctrl+S or click Review | Review the supplied backup destination, or check the selected archive before reviewing a restore |
| Ctrl+S or click Execute from review | Perform the reviewed write as a separate action |
| Esc / right-click | Return from review, close the idle panel, or request cancellation of running work |
| Wheel / PageUp / PageDown / scrollbar | Read the scope, review or result |

Enter absolute paths; the panel does not expand shell variables or `~`. Pasting a path never submits it. The backup destination must be a new archive outside the current Vault. The restore parent must already exist outside all project Vaults. The new directory is shown on completion; the app keeps the current Vault open. To recover without any working Vault, use the standalone CLI below.

Checks and writes run in one background worker. The panel holds its inputs fixed until the worker returns, retains the process writer lock, and blocks duplicate operations. Restore review records a canonical manifest hash covering paths, sizes, content hashes and runtime inclusion. Changed manifest content requires another check; every file is checked again during restoration. Checks establish integrity, not authenticity.

Cancellation is cooperative between bounded file/database steps and before publishing. The panel waits for the actual outcome and cleanup. A completed publication is reported as completed even if a cancellation arrived too late; cancellation never removes an already published backup or restored Vault. Normal panel/terminal teardown signals cancellation to an outstanding worker. Unsent Chat drafts are not part of the optional persisted-session snapshot.

## Create and verify

For CLI creation, close other PM Agent processes for this Vault. In the TUI, the background worker shares its existing writer lock. Pause edits in Obsidian or other editors for either interface. Choose an existing backup directory outside the Vault and a new filename:

```sh
pm-agent --vault /projects/example backup create --output /backups/example.pmbak
pm-agent backup check /backups/example.pmbak
```

Add `--include-sessions` to `backup create` to include the private SQLite runtime. The database must already exist; otherwise creation fails explicitly. It contains conversations, tool arguments/results, unresolved operations and a rebuildable document index. SQLite's online backup API captures committed WAL content consistently; copying a live database file alone is insufficient.

Creation obtains the normal workspace writer lock, inventories the supported scope, hashes each file and rechecks project bytes and inventory before publishing. A private temporary archive is published without overwriting any existing destination. An external editor does not participate in the lock: these checks reduce conflicts but do not create a transaction across all project files. Pause external editing for a coherent snapshot.

`backup check` reads archive structure, limits, the manifest and every included file. It does not create runtime state or modify the archive. The JSON report contains the archive path, file count, total uncompressed bytes, format, session inclusion and canonical manifest SHA-256. A checksum proves correspondence with the supplied manifest, not authorship, business correctness or authenticity. SQLite schema/integrity checks also run when restoring sessions.

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

This release reads `pm-vault-backup-v1` with `project-knowledge-v1`. Runtime snapshots must use the recognized original unversioned schema or version 1; unknown formats, scopes, schema versions and unsupported database objects fail explicitly. Restore preserves the archived runtime version. The [storage migration procedure](Storage-Migrations.md) safely adopts the known legacy schema when that runtime is subsequently opened or explicitly migrated. The manifest retains the writing application version; arbitrary historical and future versions are not supported. Local migration safety copies are excluded from project archives.

An [OKF export](../90_Agent/OKF-Profile.md) remains a knowledge distribution with transformed links and typed Skill representations. Recovery preserves working Vault bytes and native Skill adapters. Neither artifact replaces off-device storage or an exercised recovery procedure.

