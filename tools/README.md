---
type: "Reference"
language: "en"
title: "Local tools"
---

# Local tools

All 19 operations run in the Rust application through `pm-agent --vault /path/to/vault pm COMMAND ...` or the Agent's `pm` tool. Install or build the binary using [application setup](../agent/README.md). No Python runtime or model is needed for deterministic operations.

The examples assume `pm-agent` is on PATH. Otherwise substitute `agent/target/debug/pm-agent` from the framework root. `--vault` precedes `pm`; relative source paths are resolved against that Vault.

```sh
pm-agent --vault . pm settings
pm-agent --vault . pm settings --document-language en
pm-agent --vault . pm ingest /path/to/interview.pdf
pm-agent --vault . pm repo-add --id billing --path /path/to/billing --wiki openwiki --code-root src --code-root tests
pm-agent --vault . pm sync --repo billing
pm-agent --vault . pm evidence --id EVD-101 --source SRC-ID --locator page-3 --claim "A claim in the configured project language"
pm-agent --vault . pm evidence --id EVD-102 --repo billing --path src/posting.py --start 1 --end 10 --claim "Static implementation observation"
pm-agent --vault . pm prepare prd --focus BRD-001
pm-agent --vault . pm validate
pm-agent --vault . pm validate --strict
pm-agent --vault . pm coverage
pm-agent --vault . pm plan
pm-agent --vault . pm status
pm-agent --vault . pm delivery
pm-agent --vault . pm impact REQ-001
pm-agent --vault . pm questions
pm-agent --vault . pm materials
pm-agent --vault . pm materials SRC-001
pm-agent --vault . pm inspect REQ-001
pm-agent --vault . pm inspect REQ-001 --baseline BASE-001
pm-agent --vault . pm inspect EVD-102 --source
pm-agent --vault . pm fingerprint REQ-001
pm-agent --vault . pm baseline --id BASE-001 --include BRD-001 --include PRD-001 --include REQ-001 --include LANG-CONTEXT
pm-agent --vault . pm okf-export --output /path/outside-vault/new-bundle
pm-agent --vault . pm okf-check /path/to/bundle
```

Commands containing paths and IDs are interface examples; replace them with actual project inputs. The baseline command is not a complete scope. Include every applicable PRD, REQ and glossary. It prints a candidate manifest and hash, never approval. `fingerprint REQ-ID --baseline BASE-ID` reads the approved historical requirement; it requires an actual matching decision.

| Command | Result and boundary |
|---|---|
| ingest | Exact source asset plus localized draft note and extraction status; repeated import preserves edits and verifies attachment hash |
| repo-add / sync | Explicit read scope and immutable snapshot; SHA256/revision/dirty state pinned, Claims validation not performed |
| evidence | Draft material locator or line excerpt from verified snapshot bytes; never business approval |
| settings | Read/change only the project language default; no bulk translation |
| prepare | English prepared work packet; semantic work remains to be executed |
| validate | Project structure, evidence, baseline, language metadata and local OKF authoring profile |
| coverage | English operational trace report; latest matching runs, excluding simulations |
| plan | Project-language forecast from remaining effort and capacity; no optimization or delivery probability |
| status / impact | English management-state or downstream-review report; no automatic decision or external sending |
| questions | Read-only question scopes, dependency blockers and response checks; see [clarification contract](../agent/Clarification.md) |
| materials | Read-only catalog or explicit original/extraction/evidence inspection; recorded review is not verification; see [Materials](../agent/Materials.md) |
| inspect | Read-only reference chains and explicit baseline/source snapshot comparisons; no report or semantic verdict; see [inspection contract](../agent/Inspection.md) |
| delivery | Read-only structured Change/UAT/Release/Benefit overview with current problems and next-gate inputs; no generated report, state transition or approval; see [delivery contract](../agent/Delivery-Workflows.md) |
| baseline | Exact candidate snapshots; no identity or approval enforcement |
| okf-export | New read-only knowledge bundle outside the Vault; refuses an existing destination |
| okf-check | All Markdown files in the supplied bundle; core structure and optional-field warnings |

Return code 0 means the command completed (or normal validate has no errors); 1 means validation errors, or warnings under strict mode; 2 means invalid inputs or a processing failure. A report's completion does not imply business readiness. Failed forecasts replace old output with a blocked report when a renderer is available; a missing language renderer leaves old output untouched and returns an error.

Source inputs are limited to 50 MB. UTF-8 text is extracted directly; native PDF extraction preserves page locators and Office XML extraction preserves slide/document locators. Office expanded packages are limited to 200 MB and 20,000 entries, PDF pages to 10,000 with 20 MB decompressed page content, and extracted text to 50 MB. These limits do not certify arbitrary input as safe or semantically complete. Extraction failures retain the source with an explicit status.

Repository snapshots allow up to 2,000 eligible UTF-8 files, 1 MB per file and 200 MB total. Narrow scope when these limits are exceeded. Built-in secret/build-directory exclusions and simple `.openwikiignore` exclusion globs apply; complex rules require an explicitly scoped export. Linked paths are rejected or skipped without reading their targets. Source assets and snapshot files are hash-checked before evidence is created. Published bundles exclude private state and build directories; path collisions fail instead of overwriting attachments.

Sources preserve original bytes and locators. Image/scanned-PDF/chart meaning requires visual review. Multi-context terminology receives passage-level review hints, not global replacement. Missing catalogs never silently publish English project output. Settings/metadata checks cannot certify actual prose language.

See [Language Policy](../90_Agent/Language-Policy.md), [OKF profile](../90_Agent/OKF-Profile.md), [Schema](../00_Governance/Schema.md) and [OpenWiki integration](../03_Systems/OpenWiki-Integration.md).
