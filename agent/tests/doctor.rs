use std::{path::Path, process::Command};

fn write(root: &Path, path: &str, contents: &str) {
    let target = root.join(path);
    std::fs::create_dir_all(target.parent().unwrap()).unwrap();
    std::fs::write(target, contents).unwrap();
}

fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for path in [
        "00_Governance/Project.md",
        "AGENTS.md",
        "90_Agent/Evidence-and-Clarification-Policy.md",
        "90_Agent/Language-Policy.md",
        "00_Governance/Schema.md",
        "90_Agent/Playbooks/05-PRD.md",
    ] {
        write(
            dir.path(),
            path,
            "---\ntype: Reference\nlanguage: en\ntitle: Synthetic\n---\n\n# Synthetic instructions\n",
        );
    }
    write(
        dir.path(),
        "00_Governance/Settings.md",
        "---\nagent_language: en\ndocument_language: en\n---\n",
    );
    for name in ["pm-workflow", "pm-prd"] {
        write(
            dir.path(),
            &format!(".agents/skills/{name}/SKILL.md"),
            &format!(
                "---\nname: {name}\ndescription: Synthetic test Skill\n---\n\n# Synthetic instructions\n"
            ),
        );
    }
    let marker = dir.path().join("mcp-was-spawned");
    write(
        dir.path(),
        ".pm-agent/config.toml",
        &format!(
            r#"
provider = "fixture"
[providers.fixture]
protocol = "chat_completions"
base_url = "http://127.0.0.1:9/v1"
model = "fixture-model"
timeout_seconds = 1
[mcp.must_not_start]
transport = "stdio"
command = "/bin/sh"
args = ["-c", "printf invoked > \"$1\"", "fixture", {}]
"#,
            serde_json::to_string(&marker.to_string_lossy()).unwrap()
        ),
    );
    dir
}

fn doctor(root: &Path, extra: &[&str]) -> (bool, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_pm-agent"))
        .arg("--vault")
        .arg(root)
        .arg("doctor")
        .args(extra)
        .output()
        .unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    (output.status.success(), text)
}

#[test]
fn doctor_cannot_pass_a_legacy_workspace_missing_chat_governance() {
    let dir = fixture();
    let missing = "90_Agent/Evidence-and-Clarification-Policy.md";
    std::fs::remove_file(dir.path().join(missing)).unwrap();
    let (passed, output) = doctor(dir.path(), &[]);
    assert!(
        !passed,
        "Doctor falsely passed an incomplete Chat workspace:\n{output}"
    );
    assert!(
        output.contains(missing),
        "Missing-file path was omitted:\n{output}"
    );
    assert!(output.contains(dir.path().canonicalize().unwrap().to_str().unwrap()));
    assert!(!dir.path().join("mcp-was-spawned").exists());
    assert!(!dir.path().join(".pm-agent/workspace.lock").exists());
}

#[test]
fn doctor_passes_complete_selected_stage_without_network_mcp_or_runtime_writes() {
    let dir = fixture();
    for args in [vec![], vec!["--stage", "prd"]] {
        let (passed, output) = doctor(dir.path(), &args);
        assert!(passed, "{output}");
        assert!(output.contains("Configuration:"));
        assert!(output.contains("local") || output.contains("Local"));
        assert!(output.contains("Provider/MCP live compatibility has NOT been tested"));
        assert!(!dir.path().join("mcp-was-spawned").exists());
        assert!(!dir.path().join(".pm-agent/runtime.sqlite3").exists());
        assert!(!dir.path().join(".pm-agent/workspace.lock").exists());
    }
}

#[test]
fn doctor_checks_only_selected_playbook_and_collects_independent_failures() {
    let dir = fixture();
    let playbook = "90_Agent/Playbooks/05-PRD.md";
    std::fs::remove_file(dir.path().join(playbook)).unwrap();
    assert!(doctor(dir.path(), &[]).0);
    std::fs::remove_file(dir.path().join("00_Governance/Schema.md")).unwrap();
    let (passed, output) = doctor(dir.path(), &["--stage", "prd"]);
    assert!(!passed);
    assert!(output.contains(playbook), "{output}");
    assert!(output.contains("00_Governance/Schema.md"), "{output}");
    assert!(output.contains("os error 2"), "{output}");
    assert!(!dir.path().join("mcp-was-spawned").exists());
    let (passed, output) = doctor(dir.path(), &["--stage", "not-a-stage"]);
    assert!(!passed);
    assert!(output.contains("Unknown stage not-a-stage"));
}

#[test]
fn doctor_reports_missing_skill_references_and_configuration_without_hiding_other_checks() {
    let dir = fixture();
    write(
        dir.path(),
        ".agents/skills/pm-workflow/SKILL.md",
        "---\nname: pm-workflow\ndescription: Synthetic\npm_agent:\n  references:\n    - ../../../90_Agent/Missing-Reference.md\n---\n\n# Synthetic instructions\n",
    );
    std::fs::remove_file(dir.path().join(".pm-agent/config.toml")).unwrap();
    let (passed, output) = doctor(dir.path(), &[]);
    assert!(!passed);
    assert!(output.contains("Missing-Reference.md"), "{output}");
    assert!(output.contains(".pm-agent/config.toml"), "{output}");
    assert!(output.contains("Invalid Skill pm-workflow"), "{output}");
    assert!(output.contains("PASS 00_Governance/Schema.md"));
    assert!(!dir.path().join(".pm-agent/workspace.lock").exists());
}
