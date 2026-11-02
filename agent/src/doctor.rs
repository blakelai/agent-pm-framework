//! Offline checks for the same instruction inputs required by a selected Chat stage.
use crate::{
    config::Config,
    diagnostics::format_error,
    runtime::{REQUIRED_GOVERNANCE, STAGES},
    skills::Catalog,
    tools,
    workspace::Workspace,
};
use anyhow::{Context, Result, ensure};
use std::path::Path;

#[derive(Debug)]
pub struct Check {
    pub target: String,
    pub error: Option<String>,
}
#[derive(Debug, Default)]
pub struct Report {
    pub language: Option<String>,
    pub checks: Vec<Check>,
}
impl Report {
    fn record<T>(&mut self, target: &str, result: Result<T>) {
        self.checks.push(Check {
            target: target.into(),
            error: result.err().map(|error| format_error(&error)),
        });
    }
    pub fn passed(&self) -> bool {
        self.checks.iter().all(|check| check.error.is_none())
    }
    pub fn display(&self) -> String {
        self.checks
            .iter()
            .map(|check| match &check.error {
                Some(error) => format!("FAIL {}\n{}", check.target, error),
                None => format!("PASS {}", check.target),
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    }
}

/// Reads existing files only. Never creates sessions, repairs a Vault or connects to services.
pub fn check_workspace(workspace: &Workspace, stage: &str) -> Report {
    let mut report = Report::default();
    let language = workspace.language().with_context(|| {
        format!(
            "Cannot validate Chat language settings\nPath: {}",
            workspace.root.join("00_Governance/Settings.md").display()
        )
    });
    report.language = language.as_ref().ok().cloned();
    report.record("00_Governance/Settings.md", language);
    for path in REQUIRED_GOVERNANCE {
        report.record(path, workspace.read(path));
    }
    let Some((_, skill, playbook)) = STAGES.iter().find(|(name, _, _)| *name == stage) else {
        report.record::<()>(
            "Chat stage",
            Err(anyhow::anyhow!(
                "Unknown stage {stage}; choose one of: {}",
                STAGES
                    .iter()
                    .map(|(name, _, _)| *name)
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
        );
        return report;
    };
    let skill_path = format!(".agents/skills/{skill}/SKILL.md");
    // Explicitly check the selected entrypoint so an absent directory has a concrete path.
    report.record(&skill_path, workspace.read(&skill_path));
    report.record(
        "Selected Skill and declared dependencies/references",
        Catalog::discover(workspace)
            .and_then(|catalog| catalog.resolve(&[skill.to_string()]).map(|_| ()))
            .with_context(|| {
                format!(
                    "Cannot load Chat stage Skill {skill}\nPath: {}",
                    workspace.root.join(&skill_path).display()
                )
            }),
    );
    if !playbook.is_empty() {
        let path = format!("90_Agent/Playbooks/{playbook}.md");
        report.record(&path, workspace.read(&path));
    }
    report
}

/// CLI diagnostic report. All checks remain local, even when MCP servers are configured.
pub fn run(workspace: &Workspace, config_path: &Path, stage: &str) -> Result<()> {
    let config_path = std::path::absolute(config_path)
        .context("Cannot resolve the provider configuration path from the current directory")?;
    println!(
        "Workspace: {}\nConfiguration: {}\nChat stage: {stage}",
        workspace.root.display(),
        config_path.display()
    );
    let report = check_workspace(workspace, stage);
    println!("\nChat prerequisites:\n{}", report.display());
    if let Some(language) = &report.language {
        println!("Project language: {language}");
    }
    let mut passed = report.passed();
    match Config::load(&config_path) {
        Ok(config) => {
            let provider = config.active()?;
            println!(
                "Provider profile: {}\nProtocol: {:?}\nModel: {}",
                config.provider, provider.protocol, provider.model
            );
            if let Some(key) = &provider.api_key_env {
                if std::env::var(key).is_ok_and(|value| !value.is_empty()) {
                    println!("API key environment variable: present (value hidden)");
                } else {
                    passed = false;
                    println!(
                        "FAIL Missing environment variable {key}; set it in the environment that launches PM Agent. Do not paste its value into diagnostics."
                    );
                }
            }
        }
        Err(error) => {
            passed = false;
            println!("FAIL Provider configuration\n{}", format_error(&error));
        }
    }
    let commands = tools::specs()
        .into_iter()
        .find(|tool| tool.name == "pm")
        .unwrap();
    println!(
        "PM operations: native Rust ({} commands)",
        commands.parameters["properties"]["command"]["enum"]
            .as_array()
            .unwrap()
            .len()
    );
    println!(
        "Provider/MCP live compatibility has NOT been tested; no provider requests or MCP processes were started."
    );
    ensure!(
        passed,
        "Local Chat prerequisite checks failed; use the paths and causes above. Compare missing framework files with the version used by this binary, preserving project customizations and approved records."
    );
    println!("Local configuration and selected Chat stage prerequisites passed.");
    Ok(())
}
