---
type: "Reference"
title: "Material review records"
language: "en"
---

# Material review records

Record an actual review of an imported source or one material evidence claim. Each record pins the original bytes and the exact source/claim note inspected. A review records supplied observations; the application does not authenticate the reviewer, perform OCR, understand an image or approve a business requirement.

## Review through the TUI

1. In Browse press F10. Select a material and Enter/R to check its original. Inspect the original in its desktop application and the relevant extracted text and claims.
2. In Overview/Text, V starts a review of the source. In Evidence, V starts a review of the selected claim. The original must still match its archived hash. A changed target requires a fresh inspection.
3. Supply a stable review ID, title and Markdown observations in the configured project language. Drafts may leave review facts unknown. Pins are captured automatically from actual files and are not editable in the form; actor, time, coverage, method and outcome are never filled automatically.
4. To record the completed review, select workflow_status recorded and provide the fields below. Ctrl+S previews the complete proposed record; a separate Ctrl+S saves it and an English edit audit. Discard leaves the files unchanged.
5. R reloads the material. Select 4 Reviews to read its history, use [ and ] to move between records, N to open a note, or E to edit a draft. Recorded outcomes are historical. V starts another review of the selected record's current target; explicitly link previous_records when correcting an earlier result.

| Property | Meaning |
|---|---|
| coverage | full covers the entire selected source or claim; partial covers only the named scope |
| locator / review_scope | Exact location and actual scope inspected; claim locator must match the pinned evidence note |
| reviewed_by / reviewed_at | Supplied actor and actual RFC3339 time with timezone; future times are rejected |
| review_method | Actual comparison or visual inspection method |
| review_basis | original, or original_and_extraction; the latter also pins the captured extraction bytes |
| outcome | verified, follow_up or unavailable |
| findings | Actual observations supporting the result |
| follow_up | Required for partial, follow_up or unavailable results; state missing material or remaining work |
| previous_records | Explicit links to recorded reviews of the same target and source; correction time must be later |

An unavailable outcome records content that could not be understood despite intact archived bytes. Missing or damaged originals cannot establish a review in this editor: restore the archived bytes or import the supplied replacement and keep the dependent work pending. A new or changed extraction requires a new review form to capture its hash.

## Record and verification contract

Records live in 01_Sources/Reviews with type material_review and workflow_schema material-review-v1. They follow the [OKF profile](../90_Agent/OKF-Profile.md) and project language setting. The [template](../80_Templates/Material-Review.md) describes the fields; V fills only the version bindings from actual inputs. Original files, extracted text, source notes and evidence notes are never modified by the review save.

Native validation, Materials and Agent pm materials use the same assessment. Only a single current recorded review with full coverage and outcome verified clears that target's source-review check. A source review does not verify all of its claims: each claim has its own review, and a verified material claim still requires original source review. Partial or negative results, stale bytes, malformed records, unknown schemas and conflicting histories do not clear the check. The result concerns fidelity only, not requirement approval, readiness or acceptance.

Successor links determine the current recorded review. Dates alone never select a winner. Unlinked recorded reviews conflict; a new explicitly linked correction can address them while preserving history. Drafts do not supersede actual outcomes. Old review records remain visible and may report stale inputs; they are not rewritten or silently migrated. Structural errors require correction of the underlying damaged record; a new record does not conceal corrupted history.

Without new recorded review history, legacy source review_status and evidence workflow_status remain supported and are labeled legacy_verified when applicable. New recorded history takes precedence, including negative or invalid results. Legacy flags are preserved unchanged. Consumers should inspect the current review assessment instead of inferring it from those older fields.

## Boundaries and recovery

The editor rechecks targets, originals and any used extraction at preview and save. Knowledge changes clear open previews while keeping form input. Raw assets are not continuously watched; explicit inspection and save checks detect changes. These are optimistic filesystem checks, not transactions or cryptographic attestations. Editing Markdown outside the application can change history; validate flags structural and version problems but cannot prove who performed an event.

The Agent may read the records through the existing document tools or pm materials. Its draft tool cannot save material review outcomes, and no new model-write permission is introduced. Repository/OpenWiki evidence keeps its separate snapshot-based inspection contract. Review lists display up to 100 records per material. Each review preview is bounded at 16,000 UTF-8 bytes plus its truncation notice; original notes retain all fields and text. Other records remain accessible through Browse and their original notes.

Automatic vision/OCR, authenticated review signatures and semantic evaluation with real project materials remain future work. See [Materials](Materials.md), [Editing](Editing.md) and [implementation milestones](Implementation.md).
