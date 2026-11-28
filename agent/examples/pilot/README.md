---
type: "Reference"
language: "en"
title: "Synthetic procurement pilot"
---

# Synthetic procurement pilot

This example exercises the PM Agent workflow on an internal equipment purchase request. Its project documents use Traditional Chinese. The sample separates the project Vault, repository, operator answer and execution evidence.

## Create a fresh instance

Use Git, Python 3.9 or newer, a PM Agent executable and the clean framework directory unpacked from a native PM Agent package. Pass a new output directory whose parent already exists. Setup expands the Markdown templates, imports the interview, initializes a standalone repository, pins its OpenWiki and code, seeds draft Vault records, runs repository tests, and saves native validation, coverage, impact and clarification records.

```sh
bash agent/examples/pilot/setup.sh /path/to/pm-agent /path/to/package/framework /path/to/new-pilot
```

| Output | Contents |
|---|---|
| repository | Standalone Git repository with the OpenWiki, routing implementation and unit tests |
| vault | Obsidian-ready project with sources, pinned snapshots, glossary, intake BRD, PRD, requirement, proposed PBI, boundary question and test design |
| operator | Synthetic future policy to supply once the paused question has been inspected |
| checks | Actual setup checks and local repository test results |

Open the generated Vault in Obsidian or run `pm-agent --vault /path/to/new-pilot/vault tui`. Model Chat uses the operator's existing provider configuration.

## Rehearse the case

1. Inspect the generated Vault's project charter and OpenWiki. Ask Chat to compare the pinned Wiki and implementation in the registered repository, review the scoped routing question, and continue the independent intake work if needed. The Wiki and static implementation give different thresholds. The seeded records preserve both observations.
2. Confirm the routing question remains open and the routing PRD is absent. Retain the question and session identifiers as evidence.
3. Resume the Chat session and submit the held policy through the explicit answer action for QUESTION-PILOT-ROUTING. Ask Chat to draft the routing PRD under BRD-PILOT-ROUTING, followed by the requirement, PBI and a linked boundary test design. Confirm the exact PRD preview before applying it.
4. Use native validation and coverage to compare the resulting traceability with the initial intake coverage. Check the routing requirement just below, at and above the threshold confirmed by the submitted policy. Use a separate human reviewer to rehearse semantic review. Record a test result only for an actual test execution.

The repository unit tests exercise the captured implementation. The Vault records, interview and policy are synthetic. Mark trial findings as rehearsal evidence when using this example for milestone 5.
