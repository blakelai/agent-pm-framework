"""Build disposable parity cases with the existing Python planning implementation."""
import json
from pathlib import Path
import shutil
import sys

repo = Path(__file__).resolve().parents[2]
sys.path[:0] = [str(repo / "tests/legacy"), str(repo / "tests")]
from support import build_fixture
from pmkit import planning, settings
from pmkit.vault import by_id, read_note, save_note

destination = Path(sys.argv[1]).resolve()
base = destination / "base"
build_fixture(base)


def edit(root, id_, **values):
    path, meta, body = by_id(root, id_)
    meta.update(values)
    save_note(path, meta, body)


def replace(root, file, before, after):
    path = root / file
    path.write_text(path.read_text().replace(before, after))


def append(root, file, text):
    path = root / file
    path.write_text(path.read_text().rstrip() + "\n" + text + "\n")


def remaining(root, state):
    edit(root, "PBI-001", delivery_status=state)
    append(root, "05_Backlog/PBI-001.md", "\n| Role | Remaining O | Remaining M | Remaining P |\n|---|---|---|---|\n| Backend | 1 | 1 | 1 |")


def custom_locale(root):
    data = json.loads((root / "tools/locales/en.json").read_text())
    data["plan_title"] = "計画テスト {review}"
    # Escaped literal braces must remain literal, not become formatting parameters.
    data["plan_title"] = data["plan_title"].replace("{review}", "{{review}}")
    (root / "tools/locales/ja-JP.json").write_text(json.dumps(data))
    settings.update(root, "ja-JP")


people = "06_Delivery/People.md"
iterations = "06_Delivery/Iterations.md"
cases = {
    "zh_tw": lambda r: None,
    "english": lambda r: settings.update(r, "en"),
    "regional_english": lambda r: settings.update(r, "en-GB"),
    "custom_locale": custom_locale,
    "empty_schedule": lambda r: (edit(r, "PBI-001", sprint=None), edit(r, "PBI-002", sprint=None)),
    "done_cancelled": lambda r: (edit(r, "PBI-001", delivery_status="done"), edit(r, "PBI-002", delivery_status="cancelled")),
    "remaining": lambda r: remaining(r, "in_progress"),
    "blocked": lambda r: remaining(r, "blocked"),
    "zero_capacity": lambda r: replace(r, people, "| 10 | 0 |", "| 0 | 0 |"),
    "external_late": lambda r: edit(r, "PBI-001", ready_after="2026-11-01"),
    "external_wait": lambda r: edit(r, "PBI-001", ready_after="2026-10-15"),
    "deadline": lambda r: edit(r, "PBI-001", due_date="2026-10-15"),
    "dependency_unscheduled": lambda r: edit(r, "PBI-001", depends_on=["[[05_Backlog/PBI-003]]"]),
    "dependency_cancelled": lambda r: edit(r, "PBI-001", delivery_status="cancelled"),
    "dependency_same": lambda r: edit(r, "PBI-002", sprint=1),
    "dependency_later": lambda r: edit(r, "PBI-002", sprint=1, depends_on=["[[05_Backlog/PBI-001]]"]) or edit(r, "PBI-001", sprint=2),
    "short_dependency": lambda r: edit(r, "PBI-002", depends_on=["[[PBI-001#Heading|Before]]"]),
    "unconfigured_periods": lambda r: replace(r, iterations, "| 1 | 2026-10-12 | 2026-10-23 | 測試一 |\n| 2 | 2026-10-26 | 2026-11-06 | 測試二 |", ""),
    "unconfigured_as_of": lambda r: edit(r, "PLAN-CONFIG", as_of=None),
    "missing_remaining": lambda r: edit(r, "PBI-001", delivery_status="in_progress"),
    "scheduled_unknown": lambda r: edit(r, "PBI-003", sprint=1),
    "cross_project_overallocation": lambda r: append(r, people, "| test-Backend | Support | 1 | OTHER | 0.1 | 10 | 0 | 1 | 0 |"),
    "conflicting_workdays": lambda r: append(r, people, "| test-BA | Support | 1 | OTHER | 0.1 | 5 | 0 | 1 | 0 |"),
    "past_unfinished": lambda r: (edit(r, "PLAN-CONFIG", as_of="2026-11-07"), replace(r, people, "| 10 | 0 |", "| 0 | 0 |")),
    "too_many_workdays": lambda r: edit(r, "PLAN-CONFIG", as_of="2026-10-22"),
    "cycle": lambda r: edit(r, "PBI-001", depends_on=["[[05_Backlog/PBI-002]]"]),
    "non_pbi_dependency": lambda r: edit(r, "PBI-001", depends_on=["[[07_Decisions/QUESTION-001]]"]),
    "missing_dependency": lambda r: edit(r, "PBI-001", depends_on=["[[05_Backlog/ABSENT]]"]),
    "invalid_sprint_type": lambda r: edit(r, "PBI-001", sprint=True),
    "invalid_state": lambda r: edit(r, "PBI-001", delivery_status="invented"),
    "missing_capacity": lambda r: replace(r, "05_Backlog/PBI-001.md", "| Backend |", "| UnknownRole |"),
    "overlap": lambda r: replace(r, iterations, "2026-10-26", "2026-10-23"),
    "invalid_dates": lambda r: replace(r, iterations, "2026-10-23", "2026-10-01"),
    "nonfinite": lambda r: replace(r, people, "| 0.8 |", "| NaN |"),
    "absence_exceeds_allocation": lambda r: replace(r, people, "| 10 | 0 |", "| 10 | 11 |"),
    "invalid_reserve": lambda r: replace(r, people, "| 0.1 |", "| 1 |"),
    "no_current_allocation": lambda r: replace(r, people, "PROJECT-TEST", "OTHER"),
    "duplicate_allocation": lambda r: append(r, people, "| test-Backend | Backend | 1 | PROJECT-TEST | 0 | 10 | 0 | 1 | 0 |"),
    "duplicate_estimate_role": lambda r: append(r, "05_Backlog/PBI-001.md", "| Backend | 1 | 2 | 3 |"),
    "estimate_order": lambda r: replace(r, "05_Backlog/PBI-001.md", "| 2.25 | 3 | 4.5 |", "| 4 | 3 | 2 |"),
    "malformed_row": lambda r: append(r, people, "| incomplete | row |"),
}
results = []
for name, change in cases.items():
    root = destination / name
    shutil.copytree(base, root)
    change(root)
    try:
        plan = planning.calculate(root)
        report = planning.write_plan(root)
        _, body = read_note(report)
        expected = {
            "caps": [{"sprint": s, "role": role, **v} for (s, role), v in sorted(plan["caps"].items())],
            "totals": plan["totals"], "original": plan["original"], "messages": plan["messages"],
            "as_of": str(plan["as_of"]), "body": body.strip(),
        }
        results.append({"name": name, "expected": expected})
    except (ValueError, OSError, TypeError, KeyError) as exc:
        results.append({"name": name, "error": str(exc)})
(destination / "cases.json").write_text(json.dumps(results, ensure_ascii=False))
print(f"Prepared {len(results)} planning parity cases")
