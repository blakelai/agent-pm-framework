---
type: "Reference"
language: "en"
title: "OKF authoring profile"
---

# OKF authoring profile

Reference: [the user-selected OKF v0.2 specification](https://github.com/GoogleCloudPlatform/knowledge-catalog/blob/main/okf/SPEC.md), checked on 2026-10-09.

## Standard and local rules

OKF concepts are UTF-8 Markdown with YAML frontmatter and a nonempty type. The reserved index.md and log.md files have distinct navigation/history roles. Ordinary Markdown links express relationships; optional sources, generated and verified fields describe provenance and trust. Their absence is not a conformance failure. OKF concept identity is the bundle-relative path without .md. This framework's stable id is an application extension.

Our authoring profile additionally requires title and clear headings. Use a short description where it helps navigation. Keep one coherent topic per document, separate facts from proposals, link evidence and open decisions, and keep tables/code blocks readable. Preserve machine fields and add language metadata to authored project notes. Read Language Policy before drafting.

Project process state uses doc_status, delivery_status or workflow_status. Reserve status for the optional OKF knowledge lifecycle (draft/stable/deprecated). A project approval or a passing structural check is not automatically a verified event. Record verified only for an actual content verification with its actor and time. No human identity or verification is invented by migration/export.

## Scope and workflow

1. Produce project knowledge as typed Markdown. English operational instructions use the same authoring profile. Templates are typed English blueprints; instantiate project prose in document_language.
2. Keep raw originals, extracted source data, repository snapshots and immutable baselines unchanged. They are evidence assets, not newly authored concepts. Technical executables and native Skill adapters retain their runtime formats.
3. Run validate after writing. It includes the local authoring profile; broken project traceability can fail local checks even though OKF's core conformance is more permissive.
4. Use okf-export to create a new, separate bundle. Every Markdown file in that bundle is checked, including knowledge representations of all native Skills. Native SKILL.md loading adapters stay unchanged in the working Vault so the Skill host can read them.
5. Export converts body Wiki links into ordinary Markdown and exposes metadata relationships as links. Exact source .md bytes use .md.txt attachment names; export-map.json records every mapping. Snapshot manifests retain original paths and hashes; use that map when locating exported attachments. The export is a read-only knowledge distribution, not an executable replacement Vault.

The root export index supports discovery. The exporter does not add verification events or claim attested computations. Its validator checks structure and selected field guidance; it is a local implementation, not official certification or proof of semantic correctness. Current limitations include no automated semantic-language detection or trust/attestation runtime.
