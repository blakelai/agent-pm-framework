from __future__ import annotations

from .vault import catalog, link, read_note, resolve_link, save_note, stamp, content_hash
from datetime import datetime


def targets(root, refs, expected=None):
    if not isinstance(refs, list):
        raise ValueError('References must be a list')
    result = []
    for ref in refs:
        path = resolve_link(root, ref)
        meta, body = read_note(path)
        if not meta.get('id') or expected and meta.get('type') not in expected:
            raise ValueError('Invalid reference type: ' + str(ref))
        result.append((path, meta, body))
    return result


def latest_results(root, cases, reqs, runs):
    """Only complete, nonsimulated records of the exact requirement content can qualify."""
    tokens = {m['id']: m['id']+'@'+str(m['version'])+':'+content_hash(m,b) for _,m,b in reqs}
    needed = {}
    for _, m, _ in cases:
        covered = targets(root, m.get('requirements', []), {'requirement'})
        needed[m['id']] = {tokens[n['id']] for _,n,_ in covered if n['id'] in tokens}
    latest = {}
    for record in runs:
        _, m, _ = record
        if m.get('type')!='test_run' or m.get('simulation') or not all(m.get(k) for k in ['executed_at','executed_by','environment','build','evidence','requirement_versions']):
            continue
        try:
            when=datetime.fromisoformat(m['executed_at'])
            if when.tzinfo is None: continue
            evidence=targets(root,m['evidence'])
            if not evidence: continue
        except (ValueError,OSError,TypeError): continue
        for _,case,_ in targets(root,m.get('cases',[]),{'testcase'}):
            id_=case['id']
            if not needed.get(id_) or not needed[id_] <= set(m['requirement_versions']): continue
            # Equal-time conflicting results choose the nonpassing record conservatively.
            rank=(when, m.get('result')!='passed')
            if id_ not in latest or rank >= latest[id_][0]: latest[id_]=(rank,record)
    return {id_:record for id_,(_,record) in latest.items()}


def evidence_for(root, req, records):
    _, rm, rb = req
    pbis, cases, runs, releases = [], [], [], []
    for record in records:
        path, meta, body = record
        field = {'pbi': 'satisfies', 'testcase': 'requirements'}.get(meta.get('type'))
        if field and any(m['id'] == rm['id'] for _, m, _ in targets(root, meta.get(field, []), {'requirement'})):
            (pbis if field == 'satisfies' else cases).append(record)
    case_ids = {m['id'] for _, m, _ in cases}
    latest=latest_results(root,cases,[req],records)
    runs=list({record[1]['id']:record for record in latest.values() if record[1].get('result')=='passed'}.values())
    # Require every linked test case to have a passing, current result; show gaps separately.
    covered = {id_ for id_,record in latest.items() if record[1].get('result')=='passed'}
    missing_cases = case_ids - covered
    for record in records:
        _, meta, _ = record
        if meta.get('type') == 'release' and meta.get('workflow_status') == 'released' and not meta.get('simulation'):
            release_items = {m['id'] for _, m, _ in targets(root, meta.get('items', []), {'pbi'})}
            release_runs = {m['id'] for _, m, _ in targets(root, meta.get('test_runs', []), {'test_run'})}
            if pbis and all(m['id'] in release_items and m.get('delivery_status') == 'done' for _, m, _ in pbis) and runs and not missing_cases and {m['id'] for _, m, _ in runs} <= release_runs:
                releases.append(record)
    return pbis, cases, runs, releases, missing_cases


def write_report(root):
    records = catalog(root)
    target = root / '90_Agent/Reports/Traceability-Report.md'
    body = '# Atomic requirement coverage\n\nCoverage refers to current requirement versions; historical releases pin their baselines. A passing record must match version and hash and provide execution evidence. Evidence sufficiency still needs review. Simulations do not qualify.\n\n'
    body += '| Requirement | PBI | Test cases | Matching passed runs | Release | Gaps |\n|---|---|---|---|---|---|\n'
    requirements = [r for r in records if r[1].get('type') == 'requirement']
    for req in requirements:
        path, meta, _ = req
        pbis, cases, runs, releases, missing = evidence_for(root, req, records)
        gaps = []
        if not pbis: gaps.append('No PBI assigned')
        if pbis and any(m.get('delivery_status') != 'done' for _, m, _ in pbis): gaps.append('Work incomplete')
        if not cases: gaps.append('Missing test cases')
        if not runs or missing: gaps.append('Missing current-version test results')
        if not releases: gaps.append('Not released')
        names = lambda rs: '、'.join(link(root, p) for p, _, _ in rs) or '—'
        body += '| ' + ' | '.join([link(root, path), names(pbis), names(cases), names(runs), names(releases), '；'.join(gaps) or 'Coverage complete']) + ' |\n'
    if not requirements: body += '\nNo atomic requirements.\n'
    save_note(target, {'id': 'REPORT-TRACE', 'type': 'report', 'language': 'en', 'generated_at': stamp()}, body)
    return target
