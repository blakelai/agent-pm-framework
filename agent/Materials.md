---
type: "Reference"
title: "Material and evidence inspection"
language: "en"
---

# Material and evidence inspection

Use the Materials workbench to inspect imported source records, original attachments, extracted text and linked evidence without a model request. It exposes missing review inputs while retaining source bytes, language and provenance. Repository/OpenWiki snapshots remain in [Impact and Compare](Inspection.md).

## Open and navigate

In Browse, press F10 or select Materials from the document's right-click menu. Select a source, then Enter or R performs an explicit check in a background worker. Empty workspaces show no materials; ingest a supplied file first.

| Control | Action |
|---|---|
| Up/Down or mouse click | Select a material |
| Enter / R | Inspect original, extraction and linked claims |
| 1 / 2 / 3 / 4, Alt+1…4, or buttons | Overview / Text / Evidence / Reviews |
| [ / ], or Previous / Next buttons | Select another displayed claim or review |
| O / Original | Recheck and open the original in its desktop application |
| N / Note | Open the source, selected evidence claim or selected review note |
| V / Review | Create a version-bound review of the selected source or claim |
| E / Edit | Edit a draft in Reviews; recorded reviews remain historical |
| Tab / Esc | Switch focus / return to the list, then close |
| Wheel, PgUp/PgDn, Home/End, scrollbar drag | Scroll the pointed pane or details |
| Right-click a material | Select it and request inspection |
| F4 / F6 | Maximize details / toggle file-type icons |

Chat drafts are retained. Starting this view is disabled while an Agent operation is active. Original image/PDF/Office rendering uses the desktop application; terminal previews render extracted Markdown and claim text.

## CLI and Agent tool

```text
pm-agent --vault PATH pm materials
pm-agent --vault PATH pm materials SRC-ID
```

Without an ID, the command returns metadata only. With an ID, it returns a dated inspection, observed hashes, extracted text, claim locators, review history and current review assessments. The Agent uses the existing `pm` tool with `command: materials`. This read-only operation needs no model, Python, MCP or approval. It does not open an application, ingest a file, run OCR, alter review metadata or write a report. Human review creation uses the separate [review editor](Material-Reviews.md).

Inspection may complete while reporting missing files or mismatched hashes. Inspect `asset_matches`, `problems` and each claim's problems; exit code zero does not establish fidelity. Unknown or ambiguous IDs and invalid record structure fail the command.

## Evidence boundaries

Originals must be regular, unlinked files under `01_Sources/Assets`, within the existing 50 MB bound. Observed hashes are compared with source records. A failed check withholds the extraction preview. Original opening repeats note and byte checks and permits document/image/text extensions only; changes, escaping paths, links and mismatched bytes block opening.

Derived text follows the ingestion path `01_Sources/Extracted/SRC-ID.md`. Its observed hash does not prove fidelity to the original. Original language and embedded instructions remain source data. Missing, invalid UTF-8 or oversized text is unavailable. Text and claim bodies use UTF-8-safe previews of at most 16,000 bytes, labeled when truncated.

Claims are associated through their source ID or source link so conflicts remain visible. Both references must agree; pinned hashes must match and locators must be supplied. Recorded assertion types and review states remain explicit. Up to 100 linked claims are displayed with the total count; further claims remain in their original notes for direct inspection. No locator or claim meaning is inferred.

Recorded legacy `verified` fields remain visible. A separate current assessment checks explicit [review records](Material-Reviews.md), including version bindings, scope and conflicting outcomes. Byte agreement and text extraction do not establish visual understanding, semantic correctness or business approval. Inspect images, diagrams, layout, tables and scans. Record actual reviewer, date, method and reviewed scope only after that review occurs. Keep inaccessible content and dependent requirements pending; request missing material.

## Freshness and remaining work

Knowledge-document polling updates the catalog and clears completed or pending checks when project inputs change. Selection changes and superseded workers cannot attach old results to another material. Raw attachments and extracted files are not watched continuously: results identify check time, R rechecks, and O checks again before opening.

These are optimistic filesystem observations, not a transaction. Files can change after reading or after a desktop opener starts. The review editor records supplied facts; automatic vision/OCR, authenticated review attestation and semantic extraction evaluation remain future work.

See [material intake](../90_Agent/Playbooks/01-Intake.md), [source operations](../tools/README.md), [application usage](README.md) and [implementation roadmap](Implementation.md).
