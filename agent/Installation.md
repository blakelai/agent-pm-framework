---
type: "Reference"
title: "Native packages, installation and release preparation"
language: "en"
---

# Native packages, installation and release preparation

The first prebuilt targets are macOS Apple Silicon and Linux x86_64, as selected by the project owner. Packages contain the native executable, a clean Obsidian starter, configuration example, guides, local installer, package metadata and checksums. Running the application requires no Rust compiler or Python. Git remains optional for source-repository revision capture.

## Package availability and platform checks

The Native packages workflow builds and checks each target independently. Pull requests and manual workflow runs produce downloadable GitHub Actions artifacts. The outer artifact download contains a platform ZIP and its `.sha256` file. These are build artifacts, not automatically published releases.

| Package target | Native build and package verification |
|---|---|
| aarch64-apple-darwin | macOS 14 Apple Silicon runner |
| x86_64-unknown-linux-gnu | Ubuntu 22.04 x86_64 runner, glibc environment |

These are exercised environments, not certification of every OS version or Linux distribution. Intel Macs, Linux ARM64 and musl/Alpine packages are outside this first matrix. GitHub's [runner reference](https://docs.github.com/en/actions/reference/runners/github-hosted-runners) identifies available native architectures. The installer rejects a package for a different host architecture.

The macOS package is not Developer ID signed or notarized. Follow your organization's execution policy; building from reviewed source remains available. The installer does not alter quarantine or operating-system security settings. Installer prerequisites are Bash, standard Unix file utilities and either `shasum` or `sha256sum`; ZIP extraction additionally needs `unzip` or an equivalent archive application.

## Verify and install

Download the ZIP and checksum from the same trusted workflow run or reviewed release. Replace PACKAGE below with the full basename, such as `pm-agent-VERSION-REVISION-aarch64-apple-darwin`. Check that the run's revision is the intended code before executing anything.

```sh
# macOS: run in the directory containing both downloaded files.
shasum -a 256 -c PACKAGE.zip.sha256
# Linux alternative:
sha256sum -c PACKAGE.zip.sha256

unzip PACKAGE.zip
cd PACKAGE
bash install.sh --prefix "$HOME/.local"
"$HOME/.local/bin/pm-agent" --version
```

Use one checksum command for your host. A checksum detects changed bytes; it does not authenticate the publisher. The installer also checks all packaged file hashes before installation and after staging. It performs no download, asks for no elevated privilege and does not edit shell startup files. Add the prefix's `bin` directory to PATH yourself if desired.

The prefix must be an absolute physical path with no symlink components. On macOS, temporary paths may use `/var` or `/tmp` aliases; use `pwd -P` in their existing parent to obtain a physical path. Spaces in the prefix are supported. An existing unmanaged `bin/pm-agent`, linked package paths, a different payload using the same release ID, or a modified installed release causes an explicit failure.

Each release is stored under `PREFIX/lib/pm-agent/VERSION-FULL_REVISION-TARGET`. `PREFIX/bin/pm-agent` is a relative symlink to the active release. Installation uses a local directory lock and activates the verified binary by renaming a staged symlink. Reinstalling identical bytes is safe. An interrupted process can leave `.install-lock` or a staging directory under the installation; confirm that no installer is running before removing that exact leftover path. Existing release directories are retained.

## Start a project

The installer prints the retained release directory and its `framework` directory. Copy that starter into a new project directory; do not merge it into an existing project:

```sh
release="/absolute/prefix/lib/pm-agent/VERSION-FULL_REVISION-TARGET"
project="/absolute/projects/new-project"
test ! -e "$project" && cp -R "$release/framework" "$project"
pm-agent --vault "$project" pm validate
pm-agent --vault "$project" tui
```

Open the new directory in Obsidian and follow [Project setup](../00_Governance/Project-Setup.md). `PROJECT_SETUP` is expected until real project inputs are supplied. Set the document language and configure the provider separately using [application setup](README.md); blank model IDs remain blank. The package's `framework/agent/config.example.toml` is an example, not credentials or a working service configuration.

The starter includes tracked knowledge, native Skills, locale catalogs, guides and four known Obsidian settings files. It excludes runtime state, secrets, workspace-specific Obsidian files, Git history, source repositories, application source/build output and development test implementations. The test guide is retained for documentation links. Source-build instructions in those guides apply to a full repository checkout, not the reduced starter. Packaging refuses a configured project charter or records in the designated empty project folders; this is not a general secret scanner.

## Upgrade, return to an older binary, or remove

Before upgrading a working project, use [backup and restore](Backup-and-Restore.md). Verify the new archive and run its installer with the same prefix. Existing Vaults, project language, provider settings and saved sessions are not copied over by installation.

To return to a retained binary, run that release's own `install.sh --prefix SAME_PREFIX`. It verifies the retained bytes and switches the active symlink. This changes the executable only; it does not reverse schema adoption or restore documents. Check [storage compatibility](Storage-Migrations.md) before opening saved data with another version. Recover a supported backup into a separate Vault when required.

To remove the command, inspect `PREFIX/bin/pm-agent` and remove it only if it points into this installer-managed release directory. Retained releases can then be removed explicitly when no process uses them. Project Vaults and private runtime data remain separate. Automatic release cleanup and automatic updates are not implemented.

## Build and prepare a release

Maintainers build from a clean committed framework checkout on each supported native host. Cargo's locked version is the application version; the package metadata separately records the framework commit and exact binary SHA-256. The packager checks the supplied binary's native file header and reported version. The CI workflow builds that binary from the same checkout; a manually supplied binary is the maintainer's responsibility, not a provenance attestation.

```sh
cargo build --manifest-path agent/Cargo.toml --locked --release --bin pm-agent --example package
mkdir -p /absolute/output/packages
agent/target/release/examples/package --binary "$PWD/agent/target/release/pm-agent" --output /absolute/output/packages
bash agent/distribution/check.sh /absolute/output/packages/PACKAGE.zip
```

The output directory must already exist outside the checkout. Existing artifact names are never overwritten. The smoke check verifies the archive, installs and reinstalls into a disposable prefix, runs native commands against a copied starter, discovers Skills, initializes versioned storage, and exercises backup/check/restore without provider configuration. CI uploads only the ZIP/checksum pair after these checks succeed.

A maintainer-created `pm-agent-vVERSION` tag triggers the same two builds; the tag must match Cargo's version. After both package checks pass, the workflow creates a GitHub **draft** release containing their artifacts. Existing releases are not overwritten. Inspect the corresponding Framework checks, package results, licensing/distribution policy and platform requirements before publishing manually. This increment does not create a release tag, publish a release, assign a project license, configure signing credentials or establish business acceptance.
