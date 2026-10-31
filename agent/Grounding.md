---
type: "Reference"
title: "Source-to-claim review before Agent writes"
language: "en"
---

# Source-to-claim review before Agent writes

Every Agent runtime `write_draft` call first checks the existing draft contract, then requests a separate source-to-claim review from the selected provider. The reviewer receives the proposed final document and complete cited source bytes in a fresh context. It receives no author conversation, custom Skills, MCP tools or write capability. Its single response function submits a review; it cannot perform an action.

## Evidence and decisions

The runtime follows versioned `sources` links through upstream documents to their original evidence, and includes captured user answers when a cited clarification record has valid answer provenance. Upstream generated prose is a claim to review, not independent authorization. The packet pins the candidate, source versions, document language, review policy and response schema. At most 64 sources, 256 KiB of candidate plus source content and 256 nonempty body lines are accepted; oversized packets fail without truncation.

The packet also inventories every nonempty body line, including headings, tables, code and comments. The reviewer must assess each unit exactly once and explain all assertions within it. Supported units require literal quotations from supplied sources and a rationale connecting those quotations to the claims. Other classifications distinguish explicit unknowns, structural text without assertions, and items requiring clarification. Every unresolved unit needs a matching candidate quotation in the findings, and the overall decision must agree with those findings. The runtime checks coverage, quote existence and consistency; it cannot prove that a quotation logically supports a claim or that the model classified the line correctly. Frontmatter remains in the complete candidate for review and is also checked by the existing draft contract; this line inventory covers the body.

A supported review permits the existing draft writer to run after the packet is checked again for changes. A review with unsupported or uncertain assertions quotes the candidate and supplies an actual question and material request in the project language. The runtime creates and surfaces a scoped Question for that draft, keeps downstream work pending, cancels remaining parallel calls and pauses. No answer, owner, date or approval is invented. The user can supply the answer or explicitly continue independent work through the existing clarification workflow.

Invalid response schemas, omitted source or candidate coverage, invented source quotations, inconsistent decisions, unmatched candidate quotations, extra function calls, provider failure and cancellation leave the draft unwritten. A changed source, target or language requires a new review. No automatic provider retry occurs. An explicit token/evaluation guard can pause after a review; an unchanged pending draft reuses that recorded result after an explicit continuation instead of charging for the same review again.

## Accounting and evidence

Review requests count in the same durable session ledger as author requests. Only service-reported usage is recorded. Normal use still has no request-count cap. Cancellation during the read-only review is a cancelled request, not an uncertain document write.

Private session records retain review packets, candidate/source hashes, request numbers, completed model responses, per-unit assessments and parsed decisions or errors. The response must account for every supplied source path, including identical content stored at different paths; this is a coverage declaration, not proof of attention or truth. Runs list recorded outcomes; saved draft tool results reference the packet hash. Evaluations preserve these session records with their evidence hashes. Review prose is not streamed into the author's answer. Source material and stored packets can be sensitive and belong inside the project's existing access boundary.

## Limits

This is an automated model check using the same configured provider and model with an independent context. It can miss an unsupported claim or reject a valid one. Source quotations make the judgment inspectable, but related text can still be mistaken for sufficient evidence. Reviewing each line increases the response size and service-reported usage. A supported result is not truth certification, authenticated human review, project approval or readiness. Human semantic review remains separate.

The guard applies to Agent runtime draft writes. Human edits in Obsidian or typed forms, low-level library writers, historical drafts and direct deterministic PM operations are not retroactively reviewed by this mechanism. Ordinary assistant prose also remains subject to evidence policy rather than this document-write gate. Test fixtures exercise the enforcement path; live observations evaluate the selected model's judgments.

See [evidence policy](../90_Agent/Evidence-and-Clarification-Policy.md), [clarification](Clarification.md), and [evaluation](Evaluation.md).

