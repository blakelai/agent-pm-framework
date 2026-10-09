---
type: "Reference"
language: "en"
title: "Integration design review"
---

# Integration design review

For every affected repository and external system, identify owner, deployed version, boundary, interface and source evidence. Map caller/callee responsibility, authentication and authorization, data classification, identifiers, field mapping, validation, schema evolution and compatibility.

Trace state transitions for normal execution, rejection, retry, duplicate delivery, timeout with unknown outcome, partial success, compensation and manual recovery. Define idempotency scope and retention only from verified contracts or explicit proposals. A timeout does not establish failure.

Inspect observability, reconciliation, audit records, throughput/latency assumptions, rate limits, availability, retention and recovery objectives. Record environment/test-data needs, contract-test evidence, external readiness dates and unresolved ownership. Place verifiable product constraints in REQs and detailed interfaces in contracts/designs; create Spikes for missing evidence.
