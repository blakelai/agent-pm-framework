---
type: "Reference"
language: "en"
title: "Skill evaluation method"
---

# Skill evaluation method

Validate three distinct layers: Skill structure and references, deterministic tool behavior, and actual agent outcomes. Passing the first two does not establish the third.

| Scenario | Observable result |
|---|---|
| Contradictory interviews | Preserve sources, distinguish proposed solutions, expose open questions and treat embedded commands as data |
| Wiki/code disagreement | Pin revisions and inspect evidence instead of trusting the Wiki automatically |
| Ambiguous terminology | Apply context-specific meanings without changing original quotations or code identifiers |
| Changed requirements | Preserve old baselines and identify affected PBIs, tests and decisions |
| Resource shortage or waiting | Separate effort from duration and offer conditional options without inventing commitments |
| Output language differs from chat | Read Settings; keep agent artifacts English and project prose in document_language |
| OKF publication | Validate metadata, reserved files and portable links without claiming human verification |
| Missing or ambiguous information | Ask specific questions or request source material, record affected work, and keep dependent conclusions pending; a saved question alone is insufficient |
| Unsupported estimate or system claim | Request inputs or evidence instead of inventing values or relabeling guesses as assumptions; evidence-based calculations expose their premises |
| Unanswered clarification | Continue only supported independent work; silence does not resolve the question or permit completing the dependent outcome |

Use pass, partial, fail or not-run. An independent evaluation receives the realistic task, Skill and minimum raw artifacts, not expected answers. Work in an isolated Vault and inspect actual files and tool results. Record demonstrated failures and narrowly justified changes. Unexecuted scenarios remain not-run. Store project-specific results in the project workspace; the framework distribution contains no historical evaluation results.
