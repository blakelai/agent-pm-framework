---
type: "Reference"
language: "en"
title: "OpenWiki integration"
---

# OpenWiki integration

This framework reads local documentation produced by [langchain-ai/openwiki](https://github.com/langchain-ai/openwiki). Register a repository, declare its Wiki path and code roots, capture a snapshot, then create evidence and system-analysis records. The integration records revision, dirty state and SHA-256 hashes while preserving the original file structure and Claims data.

| Information | Authoritative location |
|---|---|
| Implementation, tests and API contracts | Each source repository |
| System documentation and grounded Claims | The repository's OpenWiki output |
| Business goals, PRDs, PBIs and resource plans | This project workspace |
| Business terminology | Each bounded context's CONTEXT.md |
| Versions used for analysis | Immutable snapshots and evidence records |

`sync` reads existing output; it does not generate OpenWiki, install providers or execute repository code. Use the repository's existing OpenWiki process first. When OpenWiki search tools are available, use them to locate relevant pages, then pin the corresponding local evidence before drawing conclusions.

## Scope and limitations

- Set `wiki_path` and `code_roots` explicitly. Relative repository paths are resolved inside the Vault; absolute paths may reference repositories elsewhere on the machine.
- Claims and manifest files are preserved as source assets. This adapter does not replace OpenWiki's own Claims verification or prove that documentation matches deployed behavior.
- An active `.run.json` is recorded as an incomplete-generation signal. Recent file dates do not establish semantic verification.
- `.openwikiignore` supports simple exclusion globs. Negation, escapes and character classes require a scoped export; unsupported rules stop synchronization.
- The snapshot limit is 2,000 eligible UTF-8 files and 1 MB per file. Missing or skipped scope is recorded. Default exclusions cover common secret paths and build directories; review the selected scope before importing it.
- Evidence freshness checks compare the referenced file hash. New files, cross-file behavior and deployment changes require renewed synchronization and analysis.

Keep observed behavior distinct from proposed requirements. Use [Integration review](Integration-Review.md) for contracts, failure modes and cross-system responsibilities, and [the tool manual](../tools/README.md) for commands.
