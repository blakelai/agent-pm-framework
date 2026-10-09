---
type: "Reference"
language: "en"
title: "Local tools"
---

# Local tools

Use Python 3.10+ with requirements.txt (PyYAML for OKF validation). PDF extraction also needs requirements-pdf.txt; the PDF test fixture uses reportlab. Tools do not call a model or execute source-repository code.

Run from the Vault root, or specify `--vault /path/to/vault` before the subcommand.

```sh
python3 tools/pm.py settings
python3 tools/pm.py settings --document-language en
python3 tools/pm.py ingest /path/to/interview.pdf
python3 tools/pm.py repo-add --id billing --path /path/to/billing --wiki openwiki --code-root src --code-root tests
python3 tools/pm.py sync --repo billing
python3 tools/pm.py evidence --id EVD-101 --source SRC-ID --locator page-3 --claim "A claim in the configured project language"
python3 tools/pm.py evidence --id EVD-102 --repo billing --path src/posting.py --start 1 --end 10 --claim "Static implementation observation"
python3 tools/pm.py prepare prd --focus BRD-001
python3 tools/pm.py validate
python3 tools/pm.py validate --strict
python3 tools/pm.py coverage
python3 tools/pm.py plan
python3 tools/pm.py status
python3 tools/pm.py impact REQ-001
python3 tools/pm.py fingerprint REQ-001
python3 tools/pm.py baseline --id BASE-001 --include BRD-001 --include PRD-001 --include REQ-001 --include LANG-CONTEXT
python3 tools/pm.py okf-export --output /path/outside-vault/new-bundle
python3 tools/pm.py okf-check /path/to/bundle
python3 -m unittest discover -s tests -v
```

Commands containing paths and IDs are interface examples; replace them with actual project inputs. The baseline command is not a complete scope. Include every applicable PRD, REQ and glossary. It prints a candidate manifest and hash, never approval. `fingerprint REQ-ID --baseline BASE-ID` reads the approved historical requirement; it requires an actual matching decision.

| Command | Result and boundary |
|---|---|
| settings | Read/change only the project language default; no bulk translation |
| prepare | English prepared work packet; semantic work remains to be executed |
| validate | Project structure, evidence, baseline, language metadata and local OKF authoring profile |
| coverage | English operational trace report; latest matching runs, excluding simulations |
| plan | Project-language forecast from remaining effort and capacity; no optimization or delivery probability |
| status / impact | English management-state or downstream-review report; no automatic decision or external sending |
| baseline | Exact candidate snapshots; no identity or approval enforcement |
| okf-export | New read-only knowledge bundle outside the Vault; refuses an existing destination |
| okf-check | All Markdown files in the supplied bundle; core structure and optional-field warnings |

Return code 0 means the command completed (or normal validate has no errors); 1 means validation errors, or warnings under strict mode; 2 means invalid inputs or a processing failure. A report's completion does not imply business readiness. Failed forecasts replace old output with a blocked report when a renderer is available; a missing language renderer leaves old output untouched and returns an error.

Sources preserve original bytes and locators. Image/scanned-PDF/chart meaning requires visual review. Multi-context terminology receives passage-level review hints, not global replacement. Missing catalogs never silently publish English project output. Settings/metadata checks cannot certify actual prose language.

See [Language Policy](../90_Agent/Language-Policy.md), [OKF profile](../90_Agent/OKF-Profile.md), [Schema](../00_Governance/Schema.md) and [OpenWiki integration](../03_Systems/OpenWiki-Integration.md).
