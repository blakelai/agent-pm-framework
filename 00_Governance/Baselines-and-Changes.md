---
type: "Reference"
language: "en"
title: "Baselines and change"
---

# Baselines and change

A baseline pins the content agreed at a point in time. Capturing a candidate does not approve it.

1. Select BRD, PRD, REQs and relevant glossaries; preserve versions and review evidence/differences.
2. Run baseline to capture a new non-overwritable ID, manifest and exact copies. Decision-makers review those copies or equivalent diffs.
3. Record an actual approval decision with decision_type:baseline_approval, matching baseline_id and baseline_sha256, decided_by, decided_at and decision_evidence; workflow_status becomes accepted only after that decision occurred.
4. Set doc_status:approved and baseline_id only when current content matches the approved candidate. Workflow annotations are excluded from the semantic hash; other properties, version and body are included.
5. For substantive edits or translation, preserve the old baseline and create a new versioned draft plus Change. Run impact. Existing PBIs may retain the old baseline or move following an explicit decision.

Modified text still marked approved triggers BASELINE_INVALID. A new draft differing from a PBI's execution baseline triggers NEWER_DRAFT without revoking historical approval. Interpret impact candidates semantically; a changed file does not prove every claim is invalid.

Use the baseline decision and change templates. The local files and hashes reveal accidental changes; they do not provide identity verification, signatures or tamper-proof approval. Link organizational approval records when required. Immutable Baselines are excluded from the active document catalog.
