# Repository Guidelines

## Development Scope

`pm-agent` is an application developed separately from the project-management Vault it operates on. Work in this directory follows this contributor guide. Parent Vault governance—including its `AGENTS.md`, language settings, OKF profile, approval workflows, and Run records—does not apply to application development. Consult Vault contracts when implementing compatibility with project data.

## Project Structure & Module Organization

`src/main.rs` defines the CLI; `src/lib.rs` exposes application modules. Terminal UI code lives in `src/tui/`; evaluation code lives in `src/evaluation/`. `tests/` contains Rust integration tests and Python fixtures. `examples/package.rs` and `distribution/` support native packaging. Markdown guides and `config.example.toml` document application behavior.

## Build, Test, and Development Commands

Use Rust 1.96+ and a C toolchain. Run these commands from `agent/`:

```sh
cargo build --locked                 # Build the application
cargo fmt --check                    # Check Rust formatting
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked                  # Run Rust tests and protocol fixtures
target/debug/pm-agent --vault /path/to/test-vault tui
```

For framework compatibility checks, run from the repository root: install dependencies with `python3 -m pip install -r tests/requirements.txt`, then execute `python3 -m unittest discover -s tests -v`.

## Coding Style & Naming Conventions

Use rustfmt formatting with four-space indentation. Follow existing Rust conventions: `snake_case` modules/functions, `PascalCase` types, and `SCREAMING_SNAKE_CASE` constants. Keep modules focused on their existing responsibilities. Update application guides and configuration examples when behavior changes.

## Testing Guidelines

Use Rust's `#[test]` and `#[tokio::test]` patterns, with descriptive behavior names and integration files grouped by feature. Add regression coverage for changed behavior. Use temporary Vaults and synthetic fixtures. Protocol tests need loopback listening sockets; ordinary fixture tests do not call paid providers. CI runs Rust checks on Linux/macOS and Python unittest checks. Clipboard tests require an isolated display; follow the CI workflow for opt-in execution.

## Commit & Pull Request Guidelines

Recent commits use concise imperative subjects, such as `Add configurable default Chat response language`. Follow that style. Describe the problem, resulting behavior, and actual validation results in PRs; link relevant issues and include terminal captures for visible UI changes.

## Configuration & Test Data

Keep credentials in environment variables; exclude private `.pm-agent/` state and build output from commits. Use dedicated test Vaults for manual checks that write project data. Development tasks do not require project-management Run records or Vault validation unless the change specifically concerns those features.
