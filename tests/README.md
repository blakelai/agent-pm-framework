---
type: "Reference"
title: "Development tests and legacy oracle"
language: "en"
---

# Development tests and legacy oracle

The application is native Rust. Python files under `tests/legacy` preserve the retired implementation as an independent comparison oracle, and `agent/tests` contains test-data builders and a local MCP test server. These files are not production entry points and are neither embedded in nor launched by the application.

Run development checks from the repository root:

```sh
python3 -m venv .venv
.venv/bin/python -m pip install -r tests/requirements.txt
PM_TEST_PYTHON="$PWD/.venv/bin/python" cargo test --manifest-path agent/Cargo.toml --locked
.venv/bin/python -m unittest discover -s tests -v
cargo fmt --manifest-path agent/Cargo.toml --check
cargo clippy --manifest-path agent/Cargo.toml --all-targets --locked -- -D warnings
```

The Rust suite compares planning, governance, source and validation contracts against the preserved implementation. Its fixtures create synthetic documents, repositories, PDFs and Office packages in temporary directories. PDF tests compare page locators and actual text rather than requiring identical whitespace from different extraction engines. Additional tests reject tampered snapshots, escaping paths and missing inputs. The executable is also tested with an empty PATH to establish that native operations do not start an interpreter.

Protocol tests use disposable loopback servers; they do not call a paid model. Test success does not certify a real provider, project decision, visual interpretation or business readiness. Use the [evaluation method](../90_Agent/Evals/Evaluation-Method.md) for actual Agent outcomes.

Run operational commands with the [Rust application](../agent/README.md). The retired `tools/pm.py` path and `pm --python` option no longer exist; remove the obsolete top-level `python` field from older local configurations.
