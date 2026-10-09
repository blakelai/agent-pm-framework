"""Build disposable test records; no project examples are stored in the repository."""
import shutil
from pathlib import Path

from pmkit import sources, settings, planning, governance
from pmkit.vault import read_note, save_note

ROOT = Path(__file__).resolve().parents[1]


def copy_starter(target):
    shutil.copytree(ROOT, target, ignore=shutil.ignore_patterns(
        '.git', '.venv', '__pycache__', '*.pyc', 'workspace*.json', 'exports'))


def build_fixture(target):
    copy_starter(target)
    settings.update(target, 'zh-TW')

    def note(path, id_, type_, body, **meta):
        save_note(target/path, {'id': id_, 'type': type_, 'language': 'zh-TW', **meta}, body)

    note('00_Governance/Project.md', 'PROJECT-TEST', 'project', '# 測試專案',
         owner='test-owner', workflow_status='draft')
    m,b=read_note(target/'06_Delivery/Planning.md');m['as_of']='2026-10-09'
    save_note(target/'06_Delivery/Planning.md',m,b)
    note('06_Delivery/Iterations.md', 'ITERATIONS-TEST', 'Reference',
         '# 測試期間\n\n| Sprint | Start | End | Goal |\n|---|---|---|---|\n'
         '| 1 | 2026-10-12 | 2026-10-23 | 測試一 |\n'
         '| 2 | 2026-10-26 | 2026-11-06 | 測試二 |')
    people='# 測試容量\n\n| Person | Role | Sprint | Project | FTE | Workdays | Absence | Focus | Reserve |\n|---|---|---|---|---|---|---|---|---|\n'
    for sprint in [1,2]:
        for role,fte in [('BA',.3),('Backend',1),('QA',.5),('Integration',.25),('SME',.2)]:
            people+=f'| test-{role} | {role} | {sprint} | PROJECT-TEST | {fte} | 10 | 0 | 0.8 | 0.1 |\n'
    note('06_Delivery/People.md','PEOPLE-TEST','Reference',people)

    repo=target/'01_Sources/Assets/test-repository'
    for d in ['src','openwiki/.claims']: (repo/d).mkdir(parents=True)
    (repo/'src/handler.py').write_text('def process(value):\n    return value\n')
    (repo/'openwiki/quickstart.md').write_text('# Test source\n\nprocess returns its input.\n')
    (repo/'openwiki/.claims/fixture.json').write_text('{"fixture": true}\n')
    sources.add_repo(target,'test-service',repo,'openwiki',['src'])
    sources.sync_repo(target,'test-service')
    sources.capture_evidence(target,'EVD-CODE-001','測試程式觀察',repo_id='test-service',path='src/handler.py',start=1,end=2)
    material=target/'01_Sources/Assets/test-source.txt';material.write_text('測試來源需求。\n')
    src=sources.ingest(target,material);sm,sb=read_note(src);sm['review_status']='verified';save_note(src,sm,sb)
    evidence=sources.capture_evidence(target,'EVD-USER-001','測試需求',source=sm['id'],locator='text')
    em,eb=read_note(evidence);em['workflow_status']='verified';save_note(evidence,em,eb)

    for context,term,avoid in [('alpha','測試詞','舊稱'),('beta','另一測試詞','測試詞')]:
        note(f'02_Domain/{context}/CONTEXT.md',f'LANG-{context}','glossary',
             f'# 測試語境\n\n**{term}**:\n測試定義。 ^term-test\n_Avoid_: {avoid}\n',
             context=context,version=1,doc_status='draft')
    controlled=dict(version=1,doc_status='draft',owner='test-owner',contexts=['alpha'],
                    evidence=['[[01_Sources/Evidence/EVD-USER-001]]'])
    note('04_Requirements/BRD/BRD-001.md','BRD-001','brd','# 測試問題\n\n測試詞。',**controlled)
    note('04_Requirements/PRD/PRD-001.md','PRD-001','prd','# 測試行為',
         parents=['[[04_Requirements/BRD/BRD-001]]'],**controlled)
    for i in range(1,4):
        note(f'04_Requirements/Items/REQ-00{i}.md',f'REQ-00{i}','requirement','# 測試斷言',
             parents=['[[04_Requirements/PRD/PRD-001]]'],requirement_kind='FR',**controlled)
        note(f'08_Verification/Cases/TC-00{i}.md',f'TC-00{i}','testcase','# 測試設計',
             requirements=[f'[[04_Requirements/Items/REQ-00{i}]]'],workflow_status='draft')
    note('07_Decisions/QUESTION-001.md','QUESTION-001','question','# 測試問題',
         owner='test-owner',due_date=None,workflow_status='open')
    for i in [1,2]:
        body='# 測試工作\n\n- AC-01: 可觀察的測試結果。\n- AC-02: 另一測試結果。\n\n| Role | O | M | P |\n|---|---|---|---|\n'
        for role,m in [('BA',.5),('Backend',3),('QA',2 if i==1 else 3),('Integration',2),('SME',1 if i==1 else 2)]:
            body+=f'| {role} | {m*.75} | {m:g} | {m*1.5} |\n'
        note(f'05_Backlog/PBI-00{i}.md',f'PBI-00{i}','pbi',body,owner='test-owner',contexts=['alpha'],
             evidence=controlled['evidence'],parents=['[[04_Requirements/PRD/PRD-001]]'],
             satisfies=[f'[[04_Requirements/Items/REQ-00{i}]]']+(['[[04_Requirements/Items/REQ-003]]'] if i==1 else []),
             sprint=i,depends_on=[] if i==1 else ['[[05_Backlog/PBI-001]]'],
             work_type='feature',delivery_status='proposed')
    note('05_Backlog/PBI-003.md','PBI-003','pbi',
         '# 測試研究\n\n- AC-01: 提供研究結果。\n\n| Role | O | M | P |\n|---|---|---|---|\n| Integration | | | |\n',
         owner='test-owner',contexts=['alpha'],origins=['[[07_Decisions/QUESTION-001]]'],
         research_question='測試研究問題',timebox_days=2,sprint=None,depends_on=[],
         work_type='spike',delivery_status='proposed')
    planning.write_plan(target)
    governance.status_report(target)
