"""Legacy contracts for native migration; every project record is temporary test data."""
import importlib.util
import json
from pathlib import Path
import random
import shutil
import sys

repo = Path(__file__).resolve().parents[2]
sys.path[:0] = [str(repo / "tools"), str(repo / "tests")]
from support import build_fixture
from pmkit import baselines, governance, traceability
from pmkit.vault import by_id, catalog, content_hash, digest, read_note, save_note

destination = Path(sys.argv[1]).resolve()
if len(sys.argv) > 2 and sys.argv[2] == "verify-native":
    manifest, _ = baselines.load(destination, "BASE-NATIVE")
    assert set(manifest["documents"]) == {"PRD-001", "REQ-001", "LANG-alpha"}
    print("Python accepts the native baseline without migration")
    sys.exit(0)
base = destination / "base"
build_fixture(base)


def edit(root, id_, **values):
    path, meta, body = by_id(root, id_)
    meta.update(values)
    save_note(path, meta, body)


def run(root, id_="TR-TEST", result="passed", at="2026-10-09T09:00:00+08:00", **changes):
    _, meta, body = by_id(root, "REQ-001")
    token = "REQ-001@1:" + content_hash(meta, body)
    data = dict(id=id_, type="test_run", result=result, cases=["[[08_Verification/Cases/TC-001]]"],
                requirement_versions=[token], executed_at=at, executed_by="fixture", environment="isolated-test",
                build="fixture-sha", evidence=["[[00_Governance/Project]]"], simulation=False, **{})
    data.update(changes)
    save_note(root / "08_Verification" / f"{id_}.md", data, "# Synthetic test result")


def candidate(root):
    ids = [m["id"] for _, m, _ in catalog(root) if m.get("type") in baselines.DOCUMENT_TYPES]
    return baselines.capture(root, "BASE-TEST", ids)


def approved(root):
    manifest = candidate(root)
    save_note(root / "07_Decisions/DEC-TEST.md", dict(id="DEC-TEST", type="decision", decision_type="baseline_approval",
              baseline_id="BASE-TEST", baseline_sha256=digest(manifest.read_bytes()), workflow_status="accepted",
              decided_by="synthetic fixture actor", decided_at="2026-10-09T08:00:00+08:00",
              decision_evidence=["[[00_Governance/Project]]"]), "# Synthetic approval fixture")


def changed(root, **meta):
    run(root)
    path, props, body = by_id(root, "REQ-001")
    props.update(meta)
    save_note(path, props, body + "\nChanged assertion")


def release(root, **changes):
    for id_ in ["PBI-001", "PBI-002"]:
        edit(root, id_, delivery_status="done")
    tokens = [m["id"] + "@" + str(m["version"]) + ":" + content_hash(m, b)
              for _, m, b in catalog(root) if m.get("type") == "requirement"]
    run(root, "TR-ALL", cases=[f"[[08_Verification/Cases/TC-00{i}]]" for i in [1, 2, 3]], requirement_versions=tokens)
    meta = dict(id="REL-TEST", type="release", workflow_status="released",
                items=["[[05_Backlog/PBI-001]]", "[[05_Backlog/PBI-002]]"], test_runs=["[[08_Verification/TR-ALL]]"])
    meta.update(changes)
    save_note(root / "08_Verification/REL-TEST.md", meta, "# Synthetic release fixture")


cases = {
    "drafts": lambda r: None,
    "passed": run,
    "simulated": lambda r: run(r, simulation=True),
    "missing_evidence": lambda r: run(r, evidence=[]),
    "broken_evidence": lambda r: run(r, evidence=["[[07_Decisions/ABSENT]]"]),
    "missing_executor": lambda r: run(r, executed_by=None),
    "missing_build": lambda r: run(r, build=None),
    "wrong_hash": lambda r: run(r, requirement_versions=["REQ-001@1:wrong"]),
    "malformed_versions": lambda r: run(r, requirement_versions=[{"invalid":"token"}]),
    "no_timezone": lambda r: run(r, at="2026-10-09T09:00:00"),
    "space_timestamp": lambda r: run(r, at="2026-10-09 09:00:00+08:00"),
    "later_failure": lambda r: (run(r), run(r, "TR-LATER", "failed", "2026-10-09T10:00:00+08:00")),
    "timezone_order": lambda r: (run(r), run(r, "TR-LATER", "failed", "2026-10-09T01:30:00Z")),
    "same_time_failure": lambda r: (run(r), run(r, "TR-LATER", "failed")),
    "later_pass": lambda r: (run(r, result="failed"), run(r, "TR-LATER", at="2026-10-09T10:00:00+08:00")),
    "changed_text": changed,
    "changed_version": lambda r: (run(r), edit(r, "REQ-001", version=2)),
    "workflow_annotation": lambda r: (run(r), edit(r, "REQ-001", doc_status="approved", reviewed_at="2026-10-09")),
    "all_released": release,
    "simulated_release": lambda r: release(r, simulation=True),
    "incomplete_work": lambda r: (release(r), edit(r, "PBI-001", delivery_status="in_progress")),
    "missing_release_run": lambda r: release(r, test_runs=[]),
    "additional_case": lambda r: (run(r), save_note(r / "08_Verification/Cases/TC-004.md", dict(id="TC-004",type="testcase",requirements=["[[04_Requirements/Items/REQ-001]]"]), "# Additional synthetic case")),
    "multi_requirement_case": lambda r: (run(r), edit(r, "TC-001", requirements=["[[04_Requirements/Items/REQ-001]]","[[04_Requirements/Items/REQ-002]]"])),
    "no_forecast_date": lambda r: edit(r, "PLAN-CONFIG", as_of=None),
    "overdue": lambda r: edit(r, "QUESTION-001", due_date="2026-10-08"),
    "closed_question": lambda r: edit(r, "QUESTION-001", workflow_status="resolved"),
    "candidate_only": candidate,
    "approved": approved,
    "wrong_manifest_approval": lambda r: (approved(r), edit(r,"DEC-TEST",baseline_sha256="wrong")),
    "simulated_approval": lambda r: (approved(r), edit(r,"DEC-TEST",simulation=True)),
    "rescinded_approval": lambda r: (approved(r), edit(r,"DEC-TEST",workflow_status="rescinded")),
    "new_draft_pinned_old": lambda r: (approved(r), changed(r,version=2)),
    "tampered_snapshot": lambda r: (approved(r), (r / "00_Governance/Baselines/BASE-TEST/files/REQ-001.md").write_text("tampered")),
}
results = []
for name, change in cases.items():
    root = destination / name
    shutil.copytree(base, root)
    change(root)
    expected = {"name": name}
    for operation, generate in [("coverage", traceability.write_report), ("status", governance.status_report),
                                ("impact", lambda r: baselines.impact(r,"PRD-001")), ("context_impact",lambda r: baselines.impact(r,"LANG-alpha"))]:
        try:
            path = generate(root)
            expected[operation] = read_note(path)[1].strip()
        except (ValueError,TypeError,OSError,KeyError) as exc:
            expected[operation + "_error"] = str(exc)
    _, meta, body = by_id(root,"REQ-001")
    expected["fingerprint"] = meta["id"]+"@"+str(meta["version"])+":"+content_hash(meta,body)
    if (root / "00_Governance/Baselines/BASE-TEST").exists():
        try:
            meta, body = baselines.pinned_document(root,"BASE-TEST","REQ-001")
            expected["pinned"] = meta["id"]+"@"+str(meta["version"])+":"+content_hash(meta,body)
        except (ValueError,TypeError,OSError,KeyError) as exc:
            expected["pinned_error"] = str(exc)
    results.append(expected)
(destination / "cases.json").write_text(json.dumps(results,ensure_ascii=False))

# Hash spelling is persisted data. Exercise floats, Unicode, nested values and annotation exclusions.
rng = random.Random(42)
values = [0.0,-0.0,1.0,0.0001,0.00001,1e15,1e16,1e20,1e-7,1.2345678901234567,5e-324,1.7976931348623157e308]
values += [rng.uniform(-1,1)*10**rng.randint(-300,300) for _ in range(250)]
hashes = []
for value in values:
    meta = {"id":"REQ-HASH","type":"requirement","version":1,"doc_status":"draft","data":{"數值":value,"list":[True,None,"a\nb\t雪"]}}
    body = "  # Unicode 雪\n\nAssertion.  \n"
    hashes.append({"meta":meta,"body":body,"hash":content_hash(meta,body)})
(destination / "hashes.json").write_text(json.dumps(hashes,ensure_ascii=False))
notes = []
for properties in [
    'threshold: 1e+20\nratio: 1e-07',
    'threshold: 0.000001\nratio: -0.0',
    'label: yes\ndate: 2026-10-09\nempty: null',
    "owner: 'O''Brien'\nlabel: raw text # preserved contract",
    'items:\n  - "one"\n  - 1e-07\n  - 1.5',
    'nested: {"value": 1e-07, "unicode": "雪"}',
]:
    text = '---\nid: "REQ-SCALAR"\ntype: "requirement"\nversion: 1\n' + properties + '\n---\n\n# Scalar contract\n'
    path = destination / "scalar.md"
    path.write_text(text)
    meta, body = read_note(path)
    notes.append({"text": text, "meta": meta, "body": body.strip(), "hash": content_hash(meta,body)})
(destination / "notes.json").write_text(json.dumps(notes,ensure_ascii=False))
print(f"Prepared {len(results)} governance and {len(hashes)} semantic-hash cases")
