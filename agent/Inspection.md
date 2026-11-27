---
type: "Reference"
title: "Impact and document comparison"
language: "en"
---

# Impact and document comparison

Select a managed project record with a stable ID in Browse and press F8, or choose Impact / compare from its context menu. The local inspector provides recorded downstream references and explicit text comparisons without a model call. It does not change project documents, create decisions, synchronize repositories or publish an impact report.

## Controls

| Action | Control |
|---|---|
| Impact, Baseline diff, Source diff | 1–3, Alt+1–3, Alt+Left/Right or click a tab |
| Select a record or baseline | Up/Down, wheel over the list or click a row |
| Compare the selected baseline / focus details | Enter, Compare button or right-click the row |
| Read a candidate in Browse | O or Document button |
| Scroll details | Arrow keys in details, PageUp/PageDown, wheel, scrollbar click/drag |
| Maximize / restore details | F4 or Maximize button |
| Recheck the current comparison | R or Recheck button |
| Return to list, then Browse | Esc; Back button closes the view directly |

Reading runs in a background worker. Only one inspection runs at a time; leaving and reopening during an unfinished read may require R once it completes. A late result cannot replace an invalidated or reopened view. Chat input remains intact, and inspection cannot be opened while an Agent operation is active.

## Recorded impact

The starting ID is followed through reverse Wiki links, unambiguous glossary context references, and evidence `source_id` / `repo_id` relationships. Each downstream candidate shows its type, path and one shortest reference chain, including the basis for each edge. Cycles terminate and do not list the starting record as its own candidate. Reports do not contribute outgoing references.

Unresolved or ambiguous Wiki links, source IDs and glossary contexts are listed as gaps. This list covers the scanned project, not only the starting record. Only managed active records with IDs participate. Raw files, Markdown-style links, semantic relationships and references not recorded in this graph are not inferred. Wiki-shaped text in examples may produce conservative candidates. A candidate is a review prompt, not a conclusion that a requirement or historical outcome is invalid.

## Baseline comparison

The baseline list discovers captured manifests; discovery does not verify membership, integrity or approval. Select one explicitly and compare. The reader verifies the manifest and all captured document hashes, then compares the selected historical document with its current file, including frontmatter. It shows paths, versions, manifest and file hashes, and whether the existing approval contract found a matching accepted decision.

An intact candidate can be compared without approval. Rescinded or missing approval remains visible as unverified approval with its reason. A missing member, malformed manifest, changed snapshot or unsafe path makes the comparison unavailable. The inspector never chooses a baseline by date or treats captured candidates as approved. Historical files remain unchanged.

## Source comparison

Select repository evidence, then Source diff. Its pinned file is compared with the same path in the repository registration's `latest_snapshot`. Both immutable snapshots are verified; the evidence's source hash, revision and dirty state must match its pinned manifest. The display retains snapshot IDs, capture times, revision/dirty state, assertion type and the evidence locator.

The diff covers the entire captured file, so a change may fall outside the cited lines. OpenWiki descriptions and static implementation observations keep their original evidence types. A changed file does not establish changed business needs or a failed acceptance test.

Inspection never reads the live repository. Run `pm sync --repo ID` explicitly to capture changed inputs first. An unchanged comparison means the two captured byte sequences match; it says nothing about unsynchronized files. A path absent from the latest snapshot is labeled not captured in latest scope; deletion is not established. Material evidence, images and arbitrary historical source revisions do not yet have this comparison contract.

## Freshness and limits

[Automatic file refresh](Automatic-Refresh.md) clears open inspection results when tracked project inputs change or become unreadable. R rebuilds the reference graph and revalidates comparison inputs. Repository snapshot payloads are outside automatic polling, so use R to recheck them after any external modification. Display timestamps describe the completed read, not ongoing source verification. These optimistic checks are not filesystem transactions or proof of event truth.

The line diff preserves line numbers, original line endings and final-newline flags. Added and removed lines have distinct colors; terminal control characters are displayed as escaped text. Comparisons allow up to 20,000 combined input lines. A bounded LCS calculation uses at most one million cells; larger changed regions use an explicitly labeled coarse replacement, which is exact but may not be minimal. The TUI limits diff text to 200,000 displayed characters with a visible truncation notice. The CLI returns the complete comparison within the input limits.

## Agent and CLI

These are interface examples; replace IDs with actual records:

```sh
pm-agent --vault . pm inspect REQ-001
pm-agent --vault . pm inspect REQ-001 --baseline BASE-001
pm-agent --vault . pm inspect EVD-102 --source
```

The read-only Agent `pm` operation accepts the same arguments without a new authorization prompt. `--baseline` and `--source` are mutually exclusive. Invalid comparison inputs return a failed operation and exit code 2. An unchanged file or a file not captured in the latest scope is a completed inspection with its explicit status. No report is written; the CLI can maintain its ordinary private workspace lock. The existing `pm impact ID` command still creates the established downstream review report and retains its compatibility behavior.
