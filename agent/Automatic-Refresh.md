---
type: "Reference"
title: "Automatic file refresh"
language: "en"
---

# Automatic file refresh

The TUI checks local project files every two seconds while it is open. Edits made in Obsidian or another editor update the document browser and any open Planning or Delivery view. R still requests an immediate refresh. No model connection is needed.

## What updates

Content hashes detect new, changed, renamed and deleted Markdown documents in the existing workspace knowledge scope. A replacement with the same size and modification time is still detected. Baseline Markdown/JSON payloads and local report-language catalogs also participate because they affect acceptance and planning checks. Unchanged inputs do not rebuild the index or recalculate views.

The reader runs in the background with one scan at a time. Slow scans do not queue additional work; the two-second interval is a polling cadence, not a freshness guarantee. Changed inputs are checked again after loading the views. A conflicting read is retried instead of being displayed as a coherent snapshot. Manual refreshes, saves and view changes supersede outstanding reads so an older result cannot overwrite their display.

The browser retains its category, filter, selected path, focus, layout and reading position when that path remains visible. Category bookmarks follow paths instead of old list indexes. A removed selection falls back to the first visible record and resets its reading position. Planning and Delivery retain the selected record and reading position when possible. Scroll positions are clamped to the updated rendered content.

Properties, link menus and document actions close on a refresh because their choices describe the old snapshot. An open scheduling destination picker closes with a message to reopen it against the current iteration list. Conversation input, transcript reading position, service-reported usage and active operations remain unchanged.

## Drafts and failures

An open form retains its draft and active input field. A detected input change invalidates any existing preview and requires another review before saving. The existing save checks still reject a changed source document or changed review inputs, including changes made between polls. If the record being edited changed externally, discard and reopen it to reconcile; there is no automatic merge or overwrite.

A malformed or unreadable input produces a visible Files unavailable message. The affected display snapshot and document index are cleared; old forecasts and delivery checks are not presented as current. Polling continues, and valid files restore the view without restarting. Drafts remain in memory throughout. Structural problems in readable planning/delivery records remain visible in their own views.

## Boundaries

This feature uses content polling, not operating-system filesystem notifications. It reads knowledge documents under the same scope and size limits as browsing, plus baseline payloads and local catalogs. It excludes runtime data, Git metadata, raw assets, extracted originals and repository/OpenWiki snapshot payloads. Symlinks are not followed; linked auxiliary inputs and oversized baseline/catalog files are rejected. External registered repositories are not scanned or synchronized automatically; use the existing source operations and validation for provenance checks.

Only the local document index and in-memory views change. No project report, edit audit, approval, test result, release event or measured benefit is generated. Historical reports retain their recorded timestamps until explicitly regenerated. These optimistic reads are not filesystem transactions, source-integrity certification or semantic verification. Backup/restore, document comparison and automatic conflict reconciliation remain separate milestones.

See [interaction controls](Interaction.md), [structured editing](Editing.md), [planning](Planning-View.md) and [delivery workflows](Delivery-Workflows.md).
