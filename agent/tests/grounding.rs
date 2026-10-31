//! Enforcement of auditable review coverage, not tests of model semantic accuracy.
use pm_agent::{
    grounding::{self, Packet, Source},
    provider::{Call, Completion},
    workspace::{hash, note},
};
use serde_json::{Value, json};

fn fixture() -> (Packet, Value) {
    let candidate = note(
        json!({"type":"brd","title":"Fixture","language":"en"}),
        "# Fixture\n\nThe process uses email.\nThe process has no automated validation.",
    )
    .unwrap();
    let content = "The process uses email. No evidence about automated validation is supplied.";
    let packet = Packet {
        document_language: "en".into(),
        path: "04_Requirements/BRD-TEST.md".into(),
        candidate_sha256: hash(candidate.as_bytes()),
        units: grounding::review_units(&candidate).unwrap(),
        candidate,
        sources: vec![Source {
            path: "interview.txt".into(),
            sha256: hash(content.as_bytes()),
            content: content.into(),
        }],
    };
    let review = json!({
        "decision":"needs_clarification","checked_sources":["interview.txt"],
        "assessments":[
            {"unit_id":1,"classification":"structural","rationale":"Title.","evidence":[]},
            {"unit_id":2,"classification":"supported","rationale":"Explicit current workflow.","evidence":[{"path":"interview.txt","quote":"The process uses email."}]},
            {"unit_id":3,"classification":"needs_clarification","rationale":"Email does not prove absence of validation.","evidence":[{"path":"interview.txt","quote":"No evidence about automated validation is supplied."}]}
        ],
        "findings":[{"claim":"The process has no automated validation.","reason":"The source leaves that fact unknown."}],
        "question":"What evidence establishes whether validation exists?",
        "body":"Supply the current workflow evidence; this BRD claim remains pending."
    });
    (packet, review)
}
fn parse(packet: &Packet, value: Value) -> anyhow::Result<grounding::Review> {
    grounding::parse(
        packet,
        &Completion {
            text: String::new(),
            calls: vec![Call {
                id: "review".into(),
                name: "submit_grounding_review".into(),
                arguments: value.to_string(),
            }],
            items: vec![],
            usage: None,
        },
    )
}

#[test]
fn review_requires_all_candidate_units_and_real_source_quotations() {
    let (packet, review) = fixture();
    assert!(parse(&packet, review.clone()).is_ok());
    for mutation in 0..7 {
        let mut changed = review.clone();
        match mutation {
            0 => {
                changed["assessments"].as_array_mut().unwrap().pop();
            }
            1 => changed["assessments"][2]["unit_id"] = json!(2),
            2 => changed["assessments"][1]["evidence"] = json!([]),
            3 => changed["assessments"][1]["evidence"][0]["quote"] = json!("Invented evidence"),
            4 => changed["assessments"][1]["evidence"][0]["path"] = json!("absent.txt"),
            5 => changed["assessments"][1]["evidence"][0]["quote"] = json!(" "),
            _ => changed["assessments"][1]["rationale"] = json!(" "),
        }
        assert!(parse(&packet, changed).is_err(), "mutation {mutation}");
    }
}

#[test]
fn unsupported_unit_must_match_a_finding_and_cannot_be_overridden_by_supported_decision() {
    let (packet, review) = fixture();
    let mut changed = review.clone();
    changed["decision"] = json!("supported");
    changed["findings"] = json!([]);
    changed["question"] = json!("");
    changed["body"] = json!("");
    assert!(parse(&packet, changed).is_err());
    let mut changed = review.clone();
    changed["findings"][0]["claim"] = json!("The process uses email.");
    assert!(parse(&packet, changed).is_err());
    let mut changed = review;
    changed["assessments"][2]["classification"] = json!("unknown");
    assert!(parse(&packet, changed).is_err());
}

#[test]
fn inventory_keeps_nonempty_markdown_lines_without_semantic_filtering() {
    let body = "# Asserted heading\n\n- 使用者提供的規則\n| Value |\n|---|\n```rust\nlet x = 1;\n```\n<!-- a hidden claim -->";
    let text = note(json!({"type":"Reference"}), body).unwrap();
    let units = grounding::review_units(&text).unwrap();
    assert_eq!(units.len(), 8);
    assert_eq!(units[1].text, "- 使用者提供的規則");
    assert_eq!(units[7].text, "<!-- a hidden claim -->");
    assert_eq!(
        units.iter().map(|u| u.id).collect::<Vec<_>>(),
        (1..=8).collect::<Vec<_>>()
    );
    let oversized = note(json!({"type":"Reference"}), &"Claim\n".repeat(257)).unwrap();
    assert!(grounding::review_units(&oversized).is_err());
    assert!(grounding::review_units(&note(json!({"type":"Reference"}), "").unwrap()).is_err());
}

#[test]
fn historical_reviews_remain_readable_but_cannot_satisfy_current_review_contract() {
    let (packet, mut review) = fixture();
    review.as_object_mut().unwrap().remove("assessments");
    let old_review: grounding::Review = serde_json::from_value(review.clone()).unwrap();
    assert!(old_review.assessments.is_empty());
    assert!(parse(&packet, review).is_err());
    let mut old_packet = serde_json::to_value(&packet).unwrap();
    old_packet.as_object_mut().unwrap().remove("units");
    let old_packet: Packet = serde_json::from_value(old_packet).unwrap();
    assert_ne!(
        old_packet.fingerprint().unwrap(),
        packet.fingerprint().unwrap()
    );
    assert!(parse(&old_packet, fixture().1).is_err());
}
