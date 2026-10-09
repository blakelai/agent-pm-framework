---
type: "Playbook"
language: "en"
title: "System and integration analysis"
---

# System and integration analysis

1. Read Settings and Language Policy. Inspect repository read scope, run sync, and check exclusions, Git revision and any unfinished OpenWiki run.
2. Use the Wiki index to locate relevant code, tests and contracts. Pin evidence for important claims. Static code observations do not establish deployed behavior.
3. Trace normal, rejected, duplicate, timeout, partial-success and manual-recovery paths. Use the [integration checklist](../../03_Systems/Integration-Review.md) for cross-repository responsibilities, data and state changes.
4. Record current behavior and gaps in SYS. Express desired product behavior in PRD/REQ. Record Wiki/code contradictions as questions rather than assuming the newer-looking description is correct.

Complete when important current-state claims are located, affected repositories have explicit responsibilities and interfaces, and unknowns have a verification approach. Run validate.
