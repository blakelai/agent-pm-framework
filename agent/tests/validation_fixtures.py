"""Build independent legacy validation results in disposable project copies."""
import json
from pathlib import Path
import shutil
import sys

repo = Path(__file__).resolve().parents[2]
sys.path[:0] = [str(repo / "tests/legacy"), str(repo / "tests")]
from support import build_fixture, copy_starter
from pmkit import baselines, quality
from pmkit.vault import by_id, catalog, content_hash, digest, read_note, save_note

destination = Path(sys.argv[1]).resolve()
base = destination / "base"
build_fixture(base)
if len(sys.argv) > 2 and sys.argv[2] == "base-only":
    sys.exit(0)


def edit(root, id_, **values):
    path, meta, body = by_id(root, id_)
    meta.update(values)
    if values.get("type") == "":
        path.write_text('---\n' + '\n'.join(f'{k}: {json.dumps(v, ensure_ascii=False)}' for k, v in meta.items()) + '\n---\n' + body)
    else:
        save_note(path, meta, body)
    return path, meta, body


def append(root, id_, text):
    path, meta, body = by_id(root, id_)
    save_note(path, meta, body + "\n" + text)


def approve(root, includes=None):
    includes = includes or [m["id"] for _, m, _ in catalog(root) if m.get("type") in baselines.DOCUMENT_TYPES]
    manifest = baselines.capture(root, "BASE-TEST", includes)
    save_note(root / "07_Decisions/DEC-TEST.md", dict(id="DEC-TEST", type="decision", language="zh-TW",
        decision_type="baseline_approval", baseline_id="BASE-TEST", baseline_sha256=digest(manifest.read_bytes()),
        workflow_status="accepted", decided_by="synthetic test actor", decided_at="2026-10-09T08:00:00+08:00",
        decision_evidence=["[[00_Governance/Project]]"]), "# Synthetic approval fixture")
    for id_ in includes:
        edit(root, id_, doc_status="approved", baseline_id="BASE-TEST")


def ready(root, delivery="ready"):
    approve(root)
    edit(root, "PBI-001", delivery_status=delivery, baseline_id="BASE-TEST", ready_by="synthetic test team",
         ready_at="2026-10-09", readiness_evidence=["[[00_Governance/Project]]"])


def run(root, id_="TR-AC", result="passed", **changes):
    tokens = [m["id"] + "@" + str(m["version"]) + ":" + content_hash(m, b)
              for _, m, b in catalog(root) if m.get("type") == "requirement"]
    meta = dict(id=id_, type="test_run", language="zh-TW", cases=["[[08_Verification/Cases/TC-001]]", "[[08_Verification/Cases/TC-003]]"],
        result=result, executed_at="2026-10-09T10:00:00+08:00", executed_by="fixture", environment="test", build="fixture-sha",
        evidence=["[[00_Governance/Project]]"], requirement_versions=tokens)
    meta.update(changes)
    save_note(root / f"08_Verification/{id_}.md", meta, "# Synthetic execution fixture")


def done(root):
    ready(root, "done")
    run(root)
    edit(root, "PBI-001", ac_coverage=["AC-01=TC-001", "AC-02=TC-003"],
        acceptance_evidence=["[[08_Verification/TR-AC]]"], dod_evidence=["[[00_Governance/Project]]"])


def new_draft(root):
    ready(root)
    edit(root, "REQ-001", doc_status="draft", version=2)
    append(root, "REQ-001", "A newer proposed rule.")


def source_note(root):
    return next((p, m, b) for p, m, b in catalog(root) if m.get("type") == "source")


def raw_change(root, delete=False):
    _, m, _ = source_note(root)
    p = root / m["asset"]
    if delete:
        p.unlink()
    else:
        p.write_text("Changed original fixture")


def review_pending(root):
    p, m, b = source_note(root)
    m["review_status"] = "pending"
    save_note(p, m, b)


def snapshot_change(root, **values):
    _, m, _ = by_id(root, "EVD-CODE-001")
    p = root / "03_Systems/Snapshots" / m["snapshot_id"] / "manifest.json"
    data = json.loads(p.read_text())
    data.update(values)
    p.write_text(json.dumps(data))


def break_snapshot(root):
    _, m, _ = by_id(root, "EVD-CODE-001")
    (root / "03_Systems/Snapshots" / m["snapshot_id"] / "manifest.json").write_text("{invalid JSON")


def document(root, path, text):
    p = root / path
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(text)


def glossary_ascii(root):
    append(root, "LANG-alpha", "\n**Account**:\nDefinition.\n_Avoid_: acct, ABC\n")
    append(root, "PRD-001", "ACCT is discouraged; subacct and ABCdef are not exact terms.")


cases = {
    "drafts": lambda r: None,
    "wrong_language": lambda r: edit(r, "REQ-001", language="en"),
    "bad_agent_language": lambda r: edit(r, "PROJECT-SETTINGS", agent_language="zh-TW"),
    "bad_document_language": lambda r: edit(r, "PROJECT-SETTINGS", document_language="../en"),
    "missing_renderer": lambda r: edit(r, "PROJECT-SETTINGS", document_language="ja-JP"),
    "broken_catalog": lambda r: (r / "tools/locales/zh-TW.json").write_text('{"missing":"value"}'),
    "missing_type": lambda r: edit(r, "QUESTION-001", type=""),
    "duplicate_id": lambda r: shutil.copyfile(r / "05_Backlog/PBI-001.md", r / "05_Backlog/duplicate.md"),
    "bad_parent_list": lambda r: edit(r, "REQ-001", parents="[[04_Requirements/PRD/PRD-001]]"),
    "bad_requirement_list": lambda r: edit(r, "TC-001", requirements="REQ-001"),
    "broken_link": lambda r: append(r, "PRD-001", "[[Missing-Document]]"),
    "broken_block": lambda r: append(r, "PRD-001", "[[02_Domain/alpha/CONTEXT#^absent]]"),
    "valid_block": lambda r: append(r, "PRD-001", "[[02_Domain/alpha/CONTEXT#^term-test|alias]]"),
    "fenced_link": lambda r: append(r, "PRD-001", "```\n[[Missing-Document]]\n```"),
    "terminology": lambda r: append(r, "PRD-001", "舊稱"),
    "quoted_terminology": lambda r: append(r, "PRD-001", "> 舊稱\n`舊稱`\n```\n舊稱\n```"),
    "ascii_terminology": glossary_ascii,
    "multi_context": lambda r: (edit(r, "PRD-001", contexts=["alpha", "beta"]), append(r, "PRD-001", "舊稱")),
    "unknown_context": lambda r: edit(r, "REQ-001", contexts=["unknown"]),
    "missing_owner": lambda r: edit(r, "REQ-001", owner=None),
    "missing_evidence": lambda r: edit(r, "REQ-001", evidence=[]),
    "wrong_evidence": lambda r: edit(r, "REQ-001", evidence=["[[00_Governance/Project]]"]),
    "missing_parent": lambda r: edit(r, "REQ-001", parents=[]),
    "wrong_parent": lambda r: edit(r, "REQ-001", parents=["[[00_Governance/Project]]"]),
    "bad_state": lambda r: edit(r, "REQ-001", doc_status="done", version=0),
    "bool_version": lambda r: edit(r, "REQ-001", version=True),
    "legacy_document_state": lambda r: edit(r, "REQ-001", workflow_status="draft"),
    "legacy_delivery_state": lambda r: edit(r, "PBI-001", workflow_status="draft", delivery_status="approved", work_type=None),
    "missing_atomic_requirements": lambda r: edit(r, "PBI-001", satisfies=[]),
    "bug_origin": lambda r: edit(r, "PBI-001", work_type="bug", parents=[], satisfies=[], origins=["[[04_Requirements/Items/REQ-001]]"]),
    "missing_bug_origin": lambda r: edit(r, "PBI-001", work_type="bug", origins=[]),
    "wrong_spike_origin": lambda r: edit(r, "PBI-003", origins=["[[04_Requirements/PRD/PRD-001]]"]),
    "invalid_spike": lambda r: edit(r, "PBI-003", research_question=None, timebox_days=True),
    "no_acs": lambda r: (r / "05_Backlog/PBI-003.md").write_text((r / "05_Backlog/PBI-003.md").read_text().replace("AC-01", "Criterion")),
    "missing_readiness": lambda r: edit(r, "PBI-001", delivery_status="ready"),
    "ready": ready,
    "new_draft": new_draft,
    "baseline_omission": lambda r: (approve(r, ["PRD-001"]), edit(r, "PBI-001", delivery_status="ready", baseline_id="BASE-TEST")),
    "approved_without_baseline": lambda r: edit(r, "REQ-001", doc_status="approved"),
    "approved_changed": lambda r: (approve(r), append(r, "REQ-001", "Changed approved text")),
    "unverified_evidence": lambda r: (approve(r), edit(r, "EVD-USER-001", workflow_status="draft")),
    "wrong_approval_hash": lambda r: (ready(r), edit(r, "DEC-TEST", baseline_sha256="wrong")),
    "simulated_approval": lambda r: (ready(r), edit(r, "DEC-TEST", simulation=True)),
    "rescinded_approval": lambda r: (ready(r), edit(r, "DEC-TEST", workflow_status="rescinded")),
    "tampered_baseline": lambda r: (approve(r), (r / "00_Governance/Baselines/BASE-TEST/files/REQ-001.md").write_text("tampered")),
    "done": done,
    "done_missing_dod": lambda r: (done(r), edit(r, "PBI-001", dod_evidence=[])),
    "done_missing_mapping": lambda r: (done(r), edit(r, "PBI-001", ac_coverage=["AC-01=TC-001"])),
    "done_invalid_mapping": lambda r: (done(r), edit(r, "PBI-001", ac_coverage=["bad", "AC-01=REQ-001"])),
    "done_simulation": lambda r: (done(r), edit(r, "TR-AC", simulation=True)),
    "done_failed": lambda r: (done(r), edit(r, "TR-AC", result="failed")),
    "done_wrong_hash": lambda r: (done(r), edit(r, "TR-AC", requirement_versions=["REQ-001@1:wrong"])),
    "done_unrelated_pass": lambda r: (done(r), edit(r, "TR-AC", result="failed"), run(r, "TR-UNRELATED")),
    "done_newer_draft": lambda r: (done(r), edit(r, "REQ-001", doc_status="draft", version=2), append(r, "REQ-001", "Next version")),
    "incomplete_run": lambda r: run(r, result="unknown", executed_by=None, evidence=[]),
    "missing_test_trace": lambda r: edit(r, "TC-001", requirements=[]),
    "closed_followup": lambda r: edit(r, "QUESTION-001", workflow_status="closed"),
    "source_changed": raw_change,
    "source_missing": lambda r: raw_change(r, True),
    "source_review": review_pending,
    "evidence_metadata": lambda r: edit(r, "EVD-USER-001", locator=None, assertion_type=None),
    "evidence_source_version": lambda r: edit(r, "EVD-USER-001", source_sha256="changed"),
    "evidence_missing_source": lambda r: edit(r, "EVD-USER-001", source_id="missing"),
    "evidence_no_provenance": lambda r: edit(r, "EVD-USER-001", source_id=None),
    "repo_changed_draft": lambda r: (r / "01_Sources/Assets/test-repository/src/handler.py").write_text("changed"),
    "repo_changed_verified": lambda r: (edit(r, "EVD-CODE-001", workflow_status="verified"), (r / "01_Sources/Assets/test-repository/src/handler.py").write_text("changed")),
    "repo_missing": lambda r: edit(r, "REPO-test-service", repo_path="01_Sources/Assets/missing"),
    "snapshot_mismatch": lambda r: snapshot_change(r, files={}),
    "changed_repo_and_invalid_snapshot": lambda r: ((r / "01_Sources/Assets/test-repository/src/handler.py").write_text("changed"), break_snapshot(r)),
    "openwiki_incomplete": lambda r: snapshot_change(r, openwiki_active_run="synthetic-unfinished-run"),
    "okf_no_frontmatter": lambda r: document(r, "04_Requirements/plain.md", "# Missing metadata\n"),
    "metadata_at_eof": lambda r: document(r, "04_Requirements/metadata-only.md", '---\nid: "META-ONLY"\ntype: "Reference"\ntitle: "Metadata only"\n---'),
    "okf_no_title": lambda r: document(r, "guide.md", '---\ntype: Reference\n---\n# Missing title\n'),
    "okf_wrong_status": lambda r: document(r, "guide.md", '---\ntype: Reference\ntitle: Guide\nstatus: approved\nsources: ["invalid"]\n---\n# Guide\n'),
    "okf_nested_index": lambda r: document(r, "help/index.md", '---\nokf_version: "0.2"\n---\n# Help\n- [Page](page.md)\n'),
    "okf_invalid_log": lambda r: document(r, "log.md", "# Log\n\n## 2026-99-01\n- Invalid date\n"),
    "okf_duplicate_yaml": lambda r: document(r, "guide.md", '---\ntype: Reference\ntype: Other\ntitle: Guide\n---\n# Guide\n'),
    "private_exclusions": lambda r: [document(r, p, "Not a knowledge record") for p in [".pm-agent/private.md", "target/generated.md", ".venv/helper.md", "exports/draft.md"]],
}
results = []
for name, change in cases.items():
    root = destination / name
    shutil.copytree(base, root)
    change(root)
    findings = quality.inspect(root)
    results.append({"name":name, "findings":findings})
clean = destination / "clean"
copy_starter(clean)
edit(clean, "PROJECT-NEW", workflow_status="draft")
results.append({"name":"clean", "findings":quality.inspect(clean)})
(destination / "validation.json").write_text(json.dumps(results, ensure_ascii=False))
print(f"Prepared {len(results)} independent validation scenarios")
