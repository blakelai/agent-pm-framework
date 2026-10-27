---
type: "Reference"
title: "Runtime storage versions and migration"
language: "en"
---

# Runtime storage versions and migration

PM Agent versions the private SQLite database at `.pm-agent/runtime.sqlite3`. This database stores saved conversations, tool calls and a rebuildable document index. Project Markdown, source originals, approved baselines, provider settings and business record formats are outside this migration.

## Inspect and migrate

These operator commands require an existing Vault but no model, provider configuration, MCP connection or external program:

```sh
pm-agent --vault /projects/example storage inspect
pm-agent --vault /projects/example storage migrate
```

`inspect` reports the path, state, schema version, application ID, supported version and whether migration is required. It does not initialize a missing database, acquire the workspace writer lock, migrate data or change project content. SQLite may maintain its coordination sidecars when reading a live WAL database. An unsupported schema returns an error instead of a usable-state report.

`migrate` requires an existing database and acquires the same workspace writer lock as normal PM Agent operations. Close other PM Agent writers first. It reports the resulting state and the safety snapshot path when this invocation migrated a legacy database. A current database needs no new snapshot. Normal session/workbench opening also performs the same checked migration automatically; use the explicit commands to inspect and migrate before starting work.

| State | Recognized contract | Behavior |
|---|---|---|
| Uninitialized | Missing database, or empty SQLite schema with both markers zero | Inspection creates nothing; normal opening initializes version 1; explicit migration requires a file |
| Legacy | Original three-table schema, `user_version=0`, `application_id=0` | Save a consistent private safety snapshot, then adopt version 1 transactionally |
| Current | Same three-table schema, `user_version=1`, `application_id=1347240263` (`PMAG`) | Open without migrating |
| Unsupported | Other marker combination, changed layout, unknown tables/indexes, views, triggers, or failed integrity check | Stop; preserve the database and use a compatible application or restore a supported backup into a new Vault |

The exact known table definitions and their automatic uniqueness indexes are checked, including column types and constraints. SQL keyword case and whitespace differences are allowed; additional or partial schemas are not inferred to be compatible. Version 1 records schema ownership and compatibility. It does not rewrite historical session JSON, reinterpret missing usage values, resolve uncertain tool outcomes or replay operations. Unknown serialized session fields remain opaque bytes during migration; later session loading still follows the application's own serialization contract.

## Safety sequence

The lockfile uses rusqlite 0.39.0 with bundled SQLite 3.51.3. Concurrent connection opening/closing exposed a mutex deadlock in the previous SQLite 3.51.1 dependency. The upstream [3.51.2 release notes](https://www.sqlite.org/releaselog/3_51_2.html) document the corresponding Unix lock fix. A bounded child-process regression test exercises repeated concurrent opens and migration; a deadlock fails the test instead of indefinitely blocking the suite.

1. Check the database and sidecar paths, version markers, complete schema and SQLite integrity before opening an existing unsupported store for writing.
2. Acquire a SQLite immediate transaction and recheck the schema after obtaining the writer lock. Concurrent openers cannot both migrate the same legacy state.
3. Use a separate read connection and SQLite's online backup API to capture committed data, including WAL contents, while other writers are excluded. Save the result under `.pm-agent/migrations/v0-to-v1-<unique-id>.sqlite3`.
4. Check the snapshot, flush it to disk and publish without overwriting an existing file. On macOS/Linux the directory is 0700 and the file is 0600. Snapshot failure aborts before changing schema markers.
5. Commit application ID and schema version together. A transaction failure rolls both back; an already published safety snapshot is retained. Reopening checks the actual committed state after an interruption.

SQLite lock acquisition waits up to five seconds; snapshot copy steps have a ten-second deadline. Migration snapshots do not inherit the archive's 50 MiB per-file limit. They require enough local disk space for a complete database copy and are not automatically deleted. Errors stop the operation rather than silently omitting the snapshot. Normal runtime configuration of WAL follows the committed schema transaction; a later opening/configuration error does not undo an already committed adoption.

The safety copy is private and unencrypted. It contains historical conversations and tool arguments/results, but no project files or provider configuration. It is local protection against migration failure, not an off-device backup. [Project archives](Backup-and-Restore.md) optionally capture the current runtime database; they exclude the local `migrations/` directory.

## Recover a safety snapshot

Keep the original Vault and database for diagnosis. Do not overlay an active database or mix a snapshot with another database's WAL/SHM files.

1. Use `backup restore` to recover project files into a new private Vault, without `--include-sessions`. A runtime safety snapshot alone cannot reconstruct lost project files.
2. Before opening any PM Agent process on the new Vault, create its previously absent `.pm-agent` directory with mode 0700. Copy the selected safety snapshot to `.pm-agent/runtime.sqlite3` with mode 0600. Require the destination file and its sidecars to be absent; stop if any already exist.
3. Run `storage inspect` on the new Vault. A pre-migration snapshot reports legacy state. Run `storage migrate` to adopt it with this version, preserving the original snapshot.
4. Review restored sessions and reconcile unknown external tool outcomes before resuming. Restoring or migrating does not authorize replaying a tool call.

`pm-vault-backup-v1` archives accept recognized legacy or version 1 runtime snapshots. Restore checks compatibility and preserves the archived version; migration occurs only when an operator migrates or opens that restored runtime. Unknown future versions fail before publishing a restored Vault with sessions. No arbitrary historical format, downgrade, automatic retention policy or document-format migration is supported.
