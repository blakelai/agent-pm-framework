//! Synthetic approval workflow change in an isolated Vault.
use pm_agent::{
    baselines, change_review, inspection,
    records::{self, Records},
    workspace::{Workspace, atomic_write, hash, note},
};
use serde_json::{Value, json};
use std::{fs, process::Output};

const PRD: &str = "04_Requirements/PRD-APPROVAL.md";
const EVIDENCE: &str = "01_Sources/Evidence/EVD-POLICY.md";
const DECISION: &str = "07_Decisions/DEC-APPROVAL.md";
const CASE: &str = "08_Verification/Cases/TC-EXPIRY.md";
const APPROVED: &str = "BASE-APPROVED";
const PROPOSED: &str = "BASE-PROPOSED";

fn put(w: &Workspace, path: &str, metadata: Value, body: &str) {
    atomic_write(
        &w.path(path).unwrap(),
        note(metadata, body).unwrap().as_bytes(),
    )
    .unwrap();
}

fn fixture() -> (tempfile::TempDir, Workspace) {
    let dir = tempfile::tempdir().unwrap();
    let w = Workspace {
        root: dir.path().to_path_buf(),
    };
    put(
        &w,
        "00_Governance/Project.md",
        json!({"type":"project","id":"PROJECT-TEST","language":"en"}),
        "# Synthetic project",
    );
    put(
        &w,
        "00_Governance/Settings.md",
        json!({"type":"project_settings","document_language":"en","language":"en","agent_language":"en"}),
        "# Settings",
    );
    put(
        &w,
        EVIDENCE,
        json!({
            "id":"EVD-POLICY","type":"evidence","assertion_type":"reported_need",
            "locator":"p. 2","source_id":"SRC-POLICY","source_sha256":"001122","workflow_status":"draft",
        }),
        "# Synthetic policy excerpt\n\nOne-time approval expires after a request is handled.",
    );
    let mut properties = json!({
        "type":"prd","id":"PRD-APPROVAL","title":"Tool approvals","version":1,"doc_status":"draft","language":"en",
        "owner":"Synthetic owner","contexts":["approvals"],"evidence":[EVIDENCE],
        "parents":["04_Requirements/BRD-APPROVAL.md"],
    });
    put(
        &w,
        "04_Requirements/BRD-APPROVAL.md",
        json!({"type":"brd","id":"BRD-APPROVAL","version":1}),
        "# Synthetic intent",
    );
    put(
        &w,
        PRD,
        properties.clone(),
        "# Tool approvals\n\nApproval continues for the session.",
    );
    baselines::capture(
        &Records::load(&w).unwrap(),
        APPROVED,
        &["PRD-APPROVAL".into()],
    )
    .unwrap();
    let (_, sha) = baselines::load(&w, APPROVED).unwrap();
    put(
        &w,
        DECISION,
        json!({
            "type":"decision","id":"DEC-APPROVAL","decision_type":"baseline_approval","workflow_status":"accepted",
            "baseline_id":APPROVED,"baseline_sha256":sha,"decided_at":"2026-11-01",
            "decided_by":"Synthetic actor","decision_evidence":["[[00_Governance/Project]]"],
        }),
        "# Synthetic approval event",
    );
    properties["version"] = json!(2);
    put(
        &w,
        PRD,
        properties.clone(),
        "# Tool approvals\n\nApproval expires after a single operation.",
    );
    baselines::capture(
        &Records::load(&w).unwrap(),
        PROPOSED,
        &["PRD-APPROVAL".into()],
    )
    .unwrap();
    properties["version"] = json!(3);
    put(
        &w,
        PRD,
        properties,
        "# Tool approvals\n\nApproval expires at the end of the current turn.",
    );
    put(
        &w,
        "04_Requirements/REQ-EXPIRY.md",
        json!({"id":"REQ-EXPIRY","type":"requirement","version":1,"parents":[PRD]}),
        "# Expiry requirement",
    );
    put(
        &w,
        "05_Backlog/PBI-EXPIRY.md",
        json!({"id":"PBI-EXPIRY","type":"pbi","baseline_id":APPROVED,"satisfies":["04_Requirements/REQ-EXPIRY.md"]}),
        "# Implement expiry",
    );
    put(
        &w,
        CASE,
        json!({"id":"TC-EXPIRY","type":"testcase","requirements":["04_Requirements/REQ-EXPIRY.md"]}),
        "# Check expiry",
    );
    put(
        &w,
        "08_Verification/Runs/TR-OLD.md",
        json!({"id":"TR-OLD","type":"test_run","cases":[CASE],"requirement_versions":["REQ-EXPIRY@1:synthetic"]}),
        "# Historical run",
    );
    put(
        &w,
        "05_Backlog/PBI-OTHER.md",
        json!({"id":"PBI-OTHER","type":"pbi"}),
        "# Independent work",
    );
    (dir, w)
}

fn cli(w: &Workspace, args: &[&str]) -> Output {
    std::process::Command::new(env!("CARGO_BIN_EXE_pm-agent"))
        .args([
            "--vault",
            w.root.to_str().unwrap(),
            "--config",
            "/missing-provider.toml",
            "change-review",
        ])
        .args(args)
        .env("PATH", "")
        .output()
        .unwrap()
}

fn result(output: Output) -> Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn cli_compares_captured_candidate_and_records_only_linked_observations() {
    let (_dir, w) = fixture();
    let preview = result(cli(
        &w,
        &[
            "compare",
            "PRD-APPROVAL",
            "--baseline",
            APPROVED,
            "--candidate",
            PROPOSED,
        ],
    ));
    let basis = &preview["basis"];
    assert_eq!(basis["before"]["version"], 1);
    assert_eq!(basis["candidate"]["version"], 2);
    assert_eq!(basis["candidate_manifest"]["id"], PROPOSED);
    assert_ne!(
        basis["baseline"]["sha256"],
        basis["candidate_manifest"]["sha256"]
    );
    assert_eq!(basis["approval"]["path"], DECISION);
    assert_eq!(basis["evidence"][0]["provenance"]["locator"], "p. 2");
    assert_eq!(
        basis["evidence"][0]["observed_sha256"],
        hash(w.read(EVIDENCE).unwrap().as_bytes())
    );
    let impacted = basis["affected"].as_array().unwrap();
    assert_eq!(
        impacted
            .iter()
            .map(|item| item["record"]["id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["PBI-EXPIRY", "REQ-EXPIRY", "TC-EXPIRY", "TR-OLD"]
    );
    assert_eq!(impacted[0]["execution_baseline"], APPROVED);
    assert_eq!(impacted[3]["tested_versions"][0], "REQ-EXPIRY@1:synthetic");
    assert!(
        impacted[3]["chain"][2]["basis"]
            .as_str()
            .unwrap()
            .starts_with("cases:")
    );
    let lines = preview["diff"]["lines"].as_array().unwrap();
    let candidate_line = lines
        .iter()
        .find(|line| {
            line["kind"] == "+" && line["text"] == "Approval expires after a single operation."
        })
        .unwrap()["new"]
        .as_u64()
        .unwrap();
    assert_eq!(preview["semantic_findings_status"], "pending");
    assert!(!w.path("90_Agent/Reports").unwrap().exists());
    let live = result(cli(
        &w,
        &["compare", "PRD-APPROVAL", "--baseline", APPROVED],
    ));
    assert_eq!(live["basis"]["candidate"]["version"], 3);
    assert_ne!(live["basis_sha256"], preview["basis_sha256"]);
    let inspector = inspection::load(
        &w,
        &inspection::Request {
            id: "PRD-APPROVAL".into(),
            target: inspection::Target::Review(APPROVED.into()),
        },
    )
    .unwrap();
    let inspector = inspector.change_review.unwrap();
    assert_eq!(inspector.basis_sha256, live["basis_sha256"]);
    assert!(
        inspector
            .summary()
            .contains("Recorded downstream references")
    );

    let expected = preview["basis_sha256"].as_str().unwrap();
    let input = tempfile::NamedTempFile::new().unwrap();
    let input_path = input.path();
    let mut finding = json!({
        "reviewed_by":"Synthetic reviewer",
        "findings":[{
            "side":"candidate","line":candidate_line,
            "observation":"Confirm whether a retried operation consumes a one-time approval.",
            "evidence":[EVIDENCE],"affected":["REQ-EXPIRY","PBI-EXPIRY","TC-EXPIRY"],
        }],
    });
    let record_args = [
        "record",
        "PRD-APPROVAL",
        "--baseline",
        APPROVED,
        "--candidate",
        PROPOSED,
        "--review-id",
        "FIXTURE",
        "--expect",
        expected,
        "--findings",
        input_path.to_str().unwrap(),
    ];
    let valid = finding.clone();
    for (field, invalid, error) in [
        (
            "evidence",
            json!(["01_Sources/Evidence/EVD-OTHER.md"]),
            "observed review path",
        ),
        ("affected", json!(["PBI-OTHER"]), "outside the review"),
        ("line", json!(99999), "valid compared line"),
    ] {
        finding["findings"][0][field] = invalid;
        fs::write(input_path, serde_json::to_vec(&finding).unwrap()).unwrap();
        let failure = cli(&w, &record_args);
        assert!(!failure.status.success());
        assert!(String::from_utf8_lossy(&failure.stderr).contains(error));
        assert!(!w.path("90_Agent/Reports").unwrap().exists());
        finding = valid.clone();
    }
    fs::write(input_path, serde_json::to_vec(&valid).unwrap()).unwrap();
    let receipt = result(cli(&w, &record_args));
    assert_eq!(receipt["semantic_findings_status"], "supplied");
    let report_path = receipt["path"].as_str().unwrap();
    let report = w.read(report_path).unwrap();
    assert!(report.contains("## Mechanical checks"));
    assert!(report.contains("## Semantic findings"));
    assert!(report.contains("Confirm whether a retried operation"));
    assert!(report.contains("[[01_Sources/Evidence/EVD-POLICY]]"));
    assert!(report.contains("[[08_Verification/Cases/TC-EXPIRY]]"));
    assert_eq!(receipt["sha256"], hash(report.as_bytes()));
    assert!(!cli(&w, &record_args).status.success());
    assert_eq!(w.read(report_path).unwrap(), report);
    assert!(w.read(PRD).unwrap().contains("end of the current turn"));
}

#[test]
fn stale_inputs_and_unapproved_or_damaged_baselines_cannot_be_recorded() {
    let (_dir, w) = fixture();
    let request = change_review::Request {
        id: "PRD-APPROVAL".into(),
        baseline: APPROVED.into(),
        candidate: None,
        evidence: vec![],
    };
    let baseline = change_review::load(&w, &request).unwrap();
    for path in [PRD, EVIDENCE, CASE, DECISION] {
        let original = w.read(path).unwrap();
        atomic_write(
            &w.path(path).unwrap(),
            format!("{original}\nUpdated fixture input.\n").as_bytes(),
        )
        .unwrap();
        let error =
            change_review::record(&w, &request, "STALE", &baseline.basis_sha256, None).unwrap_err();
        assert!(
            error.to_string().contains("Review basis changed"),
            "{error:#}"
        );
        atomic_write(&w.path(path).unwrap(), original.as_bytes()).unwrap();
    }
    assert!(!w.path("90_Agent/Reports").unwrap().exists());
    let pending =
        change_review::record(&w, &request, "PENDING", &baseline.basis_sha256, None).unwrap();
    assert_eq!(pending["semantic_findings_status"], "pending");
    assert!(
        w.read(pending["path"].as_str().unwrap())
            .unwrap()
            .contains("Pending.")
    );

    let mut d = records::read(&w, DECISION).unwrap();
    d.metadata["workflow_status"] = json!("rescinded");
    put(&w, DECISION, d.metadata, &d.body);
    let failure = cli(&w, &["compare", "PRD-APPROVAL", "--baseline", APPROVED]);
    assert!(!failure.status.success());
    assert!(String::from_utf8_lossy(&failure.stderr).contains("rescinded"));
    let failure = cli(&w, &["compare", "PRD-APPROVAL", "--baseline", PROPOSED]);
    assert!(!failure.status.success());
    assert!(String::from_utf8_lossy(&failure.stderr).contains("No accepted decision"));
    fs::write(
        w.path("00_Governance/Baselines/BASE-APPROVED/files/PRD-APPROVAL.md")
            .unwrap(),
        "# damaged",
    )
    .unwrap();
    assert!(change_review::load(&w, &request).is_err());
}

#[test]
fn structural_findings_surface_version_and_evidence_gaps_without_a_semantic_verdict() {
    let (_dir, w) = fixture();
    let mut d = records::read(&w, PRD).unwrap();
    d.metadata["version"] = json!(1);
    d.metadata["sources"] = json!([
        {"resource":EVIDENCE,"sha256":"changed"},
        {"resource":"01_Sources/Evidence/missing.md","sha256":"missing"},
    ]);
    put(&w, PRD, d.metadata, &d.body);
    let review = result(cli(
        &w,
        &[
            "compare",
            "PRD-APPROVAL",
            "--baseline",
            APPROVED,
            "--evidence",
            "EVD-POLICY",
        ],
    ));
    let findings = review["basis"]["mechanical_findings"].as_array().unwrap();
    assert!(
        findings
            .iter()
            .any(|f| f["level"] == "ERROR" && f["code"] == "CHANGE_VERSION")
    );
    assert!(findings.iter().any(|f| {
        f["code"] == "REVIEW_EVIDENCE"
            && f["message"]
                .as_str()
                .unwrap()
                .contains("Declared SHA256 differs")
    }));
    assert!(
        findings.iter().any(|f| f["code"] == "REVIEW_EVIDENCE"
            && f["message"].as_str().unwrap().contains("missing.md"))
    );
    assert!(
        review["basis"]["evidence"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["side"] == "additional")
    );
    assert_eq!(review["semantic_findings_status"], "pending");
}
