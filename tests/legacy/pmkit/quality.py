from __future__ import annotations

import json
import re

from .sources import repo_location
from . import baselines, settings, okf
from .traceability import targets, latest_results
from .vault import content_hash
from .vault import by_id, catalog, contained, digest, link, managed_notes, read_note, resolve_link, save_note, stamp


def inspect(root):
    findings, records, ids = [], [], {}
    def issue(level, code, path, message):
        findings.append({'level': level, 'code': code, 'file': str(path.relative_to(root)), 'message': message})
    try:
        config = settings.load(root)
    except (ValueError,OSError) as exc:
        issue('ERROR','LANGUAGE_SETTINGS',root/settings.SETTINGS_PATH,str(exc)); config=None
    if config:
        try: settings.catalog(root)
        except (ValueError,OSError) as exc: issue('WARN','LANGUAGE_CATALOG',root/settings.SETTINGS_PATH,str(exc))
    for path in managed_notes(root):
        try:
            meta, body = read_note(path)
        except (ValueError, OSError) as exc:
            issue('ERROR', 'PROPERTIES', path, str(exc))
            continue
        if not meta.get('id'):
            continue
        records.append((path, meta, body))
        id_ = meta['id']
        if id_ in ids:
            issue('ERROR', 'DUPLICATE_ID', path, id_)
        ids[id_] = path
        if not meta.get('type'):
            issue('ERROR', 'TYPE', path, 'Missing type')
        for field in ['parents', 'evidence', 'depends_on', 'contexts', 'satisfies', 'origins', 'requirements', 'cases', 'requirement_versions', 'items', 'test_runs', 'ac_coverage']:
            if field in meta and not isinstance(meta[field], list):
                issue('ERROR', 'PROPERTY_TYPE', path, field + ' must be a list')
        combined = json.dumps(meta, ensure_ascii=False) + '\n' + re.sub(r'```.*?```', '', body, flags=re.S)
        for match in re.finditer(r'\[\[([^\]]+)\]\]', combined):
            value = match.group(1)
            try:
                target = resolve_link(root, value)
                if '#^' in value:
                    anchor = value.split('#^', 1)[1].split('|', 1)[0]
                    if '^' + anchor not in target.read_text(encoding='utf-8'):
                        raise ValueError('Block reference not found')
            except (ValueError, OSError) as exc:
                issue('ERROR', 'LINK', path, str(exc))

    contexts = {}
    for path, meta, body in records:
        if meta.get('type') == 'glossary':
            terms = []
            for term, block in re.findall(r'\*\*([^*]+)\*\*:\s*\n(.*?)(?=\n\*\*|\Z)', body, re.S):
                avoid = re.search(r'^_Avoid_:\s*(.+)$', block, re.M)
                terms.append((term, re.split(r'[,，、]', avoid.group(1)) if avoid else []))
            contexts[meta.get('context')] = (meta, terms)

    for path, meta, body in records:
        type_ = meta.get('type')
        if config and type_ in {'brd','prd','requirement','glossary','pbi','system_analysis','testcase','decision','contract','design'}:
            expected_language = settings.required_language(root,path.relative_to(root))
            if meta.get('language') != expected_language:
                issue('WARN','DOCUMENT_LANGUAGE',path,'Declared language is missing or differs from '+expected_language+'; review or migrate the authorized scope without changing historical baselines')
        state = meta.get('workflow_status')
        if type_ == 'project' and state == 'unconfigured':
            issue('WARN','PROJECT_SETUP',path,'Complete Project setup; no scope, resource commitments or delivery baseline have been recorded')
        if type_ in baselines.DOCUMENT_TYPES:
            if meta.get('doc_status') not in {'draft','in_review','approved','retired'} or type(meta.get('version')) is not int or meta.get('version',0) < 1:
                issue('ERROR','DOCUMENT_STATE',path,'Controlled documents need doc_status and a positive integer version')
            if 'workflow_status' in meta:
                issue('ERROR','LEGACY_STATUS',path,'Use doc_status for controlled-document state')
        if type_ == 'pbi':
            if meta.get('delivery_status') not in {'proposed','ready','in_progress','blocked','done','cancelled'}:
                issue('ERROR','DELIVERY_STATE',path,'Invalid delivery_status')
            if 'workflow_status' in meta:
                issue('ERROR','LEGACY_STATUS',path,'Use delivery_status for PBI execution state')
            if meta.get('work_type') not in {'feature','bug','enabler','spike','migration'}:
                issue('ERROR','WORK_TYPE',path,'Specify work_type')
        if type_ in {'brd','prd','requirement','pbi'}:
            controlled = meta.get('doc_status') == 'approved'
            delivery = meta.get('delivery_status')
            active = type_ == 'pbi' and delivery in {'ready','in_progress','blocked','done'}
            if not meta.get('owner') or not meta.get('contexts'):
                issue('ERROR','OWNERSHIP',path,'Specify owner and contexts')
            evidence = meta.get('evidence') if isinstance(meta.get('evidence'),list) else []
            if type_ != 'pbi' and not evidence:
                issue('ERROR','EVIDENCE_REQUIRED',path,'Missing evidence links')
            expected_parent = {'prd':'brd','requirement':'prd'}.get(type_)
            if type_ == 'pbi' and meta.get('work_type') in {'feature','migration'}:
                expected_parent = 'prd'
            parents = meta.get('parents') if isinstance(meta.get('parents'),list) else []
            if expected_parent and not parents:
                issue('ERROR','TRACEABILITY',path,'Missing upstream '+expected_parent)
            try:
                parent_records = targets(root,parents,{expected_parent} if expected_parent else None)
            except (ValueError,OSError) as exc:
                issue('ERROR','PARENT_TYPE',path,str(exc)); parent_records=[]
            for ref in evidence:
                try:
                    em,_=read_note(resolve_link(root,ref))
                    if em.get('type')!='evidence': issue('ERROR','EVIDENCE_TYPE',path,'Evidence field must link to EVD')
                    elif controlled and em.get('workflow_status')!='verified': issue('ERROR','UNVERIFIED_EVIDENCE',path,em['id'])
                except (ValueError,OSError): pass
            note_contexts = meta.get('contexts') if isinstance(meta.get('contexts'),list) else []
            multi_context = len(set(note_contexts)) > 1
            if multi_context:
                issue('WARN','MULTI_CONTEXT_REVIEW',path,'Review terminology by passage in this multi-context document; global replacements are not inferred')
            for context in note_contexts:
                if context not in contexts:
                    issue('ERROR','CONTEXT',path,'Unknown context: '+str(context)); continue
                gm,terms=contexts[context]
                if controlled and gm.get('doc_status')!='approved':
                    issue('ERROR','GLOSSARY_UNCONFIRMED',path,'Glossary is not approved: '+context)
                if multi_context:
                    continue
                authored=re.sub(r'```.*?```|`[^`]+`','',body,flags=re.S)
                authored='\n'.join(line for line in authored.splitlines() if not line.startswith('>'))
                for canonical,avoids in terms:
                    for avoid in (v.strip() for v in avoids):
                        if not avoid or avoid=='無': continue
                        pattern=r'(?<!\w)'+re.escape(avoid)+r'(?!\w)' if avoid.isascii() else re.escape(avoid)
                        if re.search(pattern,authored,re.I): issue('WARN','TERMINOLOGY',path,f'{context}: review term "{avoid}"; suggested canonical term "{canonical}"')
            if type_=='pbi':
                work=meta.get('work_type')
                reqs=[]
                try: reqs=targets(root,meta.get('satisfies',[]),{'requirement'})
                except (ValueError,OSError) as exc: issue('ERROR','REQUIREMENT_LINK',path,str(exc))
                if work in {'feature','migration'} and not reqs: issue('ERROR','REQUIREMENT_LINK',path,'Feature/migration work needs atomic requirements')
                if work in {'bug','enabler','spike'}:
                    allowed={'bug':{'issue','requirement'},'enabler':{'system_analysis','design','decision','requirement','question'},'spike':{'question'}}[work]
                    try:
                        if not targets(root,meta.get('origins',[]),allowed): raise ValueError('Missing work origin')
                    except (ValueError,OSError) as exc: issue('ERROR','WORK_ORIGIN',path,str(exc))
                if work=='spike' and (not meta.get('research_question') or type(meta.get('timebox_days')) not in (int,float) or meta.get('timebox_days',0)<=0):
                    issue('ERROR','SPIKE_CONTRACT',path,'Spike needs a research question and positive timebox_days')
                acs=set(re.findall(r'AC-\d+',body))
                if not acs: issue('ERROR','AC_REQUIRED',path,'Provide numbered outcome/acceptance criteria')
                if active:
                    if not all(meta.get(k) for k in ['ready_by','ready_at','readiness_evidence']):
                        issue('ERROR','READINESS_RECORD',path,'Provide an actual team readiness record')
                    if work in {'feature','migration'}:
                        try:
                            baseline=meta.get('baseline_id')
                            if not baseline: raise ValueError('Executing feature/migration work must pin baseline_id')
                            manifest,_=baselines.approval(root,baseline)
                            required_ids={m['id'] for _,m,_ in parent_records+reqs}
                            required_ids.update(contexts[c][0]['id'] for c in meta.get('contexts',[]) if c in contexts)
                            if not required_ids <= manifest['documents'].keys(): raise ValueError('Baseline must include PRD, requirements and applicable glossaries')
                            for _,rm,rb in reqs:
                                entry=manifest['documents'][rm['id']]
                                if entry['content_sha256']!=content_hash(rm,rb):
                                    issue('WARN','NEWER_DRAFT',path,'Current requirement differs from the execution baseline; PBI remains pinned to '+baseline)
                        except (ValueError,OSError,KeyError,TypeError) as exc: issue('ERROR','EXECUTION_BASELINE',path,str(exc))
                if delivery=='done':
                    for field in ['acceptance_evidence','dod_evidence']:
                        try:
                            if not targets(root,meta.get(field,[])): raise ValueError('Missing '+field)
                        except (ValueError,OSError) as exc: issue('ERROR','DONE_EVIDENCE',path,str(exc))
                    if work in {'feature','migration','bug'}:
                        mapped=set()
                        for item in meta.get('ac_coverage',[]):
                            bits=str(item).split('=',1)
                            if len(bits)!=2 or bits[0] not in acs:
                                issue('ERROR','AC_COVERAGE',path,'Use AC mapping format AC-01=TC-001'); continue
                            try:
                                _,tm,_=by_id(root,bits[1])
                                if tm.get('type')!='testcase': raise ValueError('AC must map to a test case')
                                mapped.add(bits[0])
                            except ValueError as exc: issue('ERROR','AC_COVERAGE',path,str(exc))
                        if mapped!=acs: issue('ERROR','AC_COVERAGE',path,'Not all acceptance criteria are covered')
                        if work in {'feature','migration'} and reqs:
                            try:
                                pinned=[]
                                for rp,rm,rb in reqs:
                                    pm,pb=baselines.pinned_document(root,meta.get('baseline_id'),rm['id'])
                                    pinned.append((rp,pm,pb))
                                case_ids={str(x).split('=',1)[1] for x in meta.get('ac_coverage',[]) if '=' in str(x)}
                                cases=[by_id(root,id_) for id_ in case_ids]
                                runs=targets(root,meta.get('acceptance_evidence',[]),{'test_run'})
                                latest=latest_results(root,cases,pinned,runs)
                                if set(latest)!=case_ids or any(r[1].get('result')!='passed' for r in latest.values()):
                                    raise ValueError('AC lacks an actual passing run for its execution baseline')
                            except (ValueError,OSError,TypeError,KeyError) as exc:
                                issue('ERROR','DONE_TEST_RESULTS',path,str(exc))
            if meta.get('doc_status')=='draft' or delivery=='proposed':
                issue('WARN','DRAFT',path,'Draft/proposed work is not a team delivery commitment')
        if type_=='testcase':
            try:
                if not targets(root,meta.get('requirements',[]),{'requirement'}): raise ValueError('Test case must link to requirements')
            except (ValueError,OSError) as exc: issue('ERROR','TEST_TRACE',path,str(exc))
        if type_=='test_run':
            if meta.get('result') not in {'passed','failed','blocked'}: issue('ERROR','TEST_RESULT',path,'Invalid test result')
            try:
                if not targets(root,meta.get('cases',[]),{'testcase'}): raise ValueError('Test run must specify cases')
            except (ValueError,OSError) as exc: issue('ERROR','TEST_TRACE',path,str(exc))
            if not all(meta.get(k) for k in ['executed_at','executed_by','environment','build','evidence','requirement_versions']):
                issue('ERROR','TEST_RECORD',path,'Missing execution time, actor, environment, build, evidence or requirement versions')
        if type_ in {'risk','issue','action','question'} and meta.get('workflow_status') not in {'closed','resolved'}:
            if not meta.get('owner') or not meta.get('due_date'): issue('WARN','FOLLOWUP_OWNER',path,'Open follow-up needs an owner and due date')
        if type_ == 'source':
            try:
                raw = contained(root, meta['asset'])
                if digest(raw.read_bytes()) != meta.get('sha256'):
                    issue('ERROR', 'SOURCE_CHANGED', path, 'Archived source hash mismatch')
            except (ValueError, OSError, KeyError) as exc:
                issue('ERROR', 'SOURCE_MISSING', path, str(exc))
            if meta.get('review_status') != 'verified':
                issue('WARN', 'SOURCE_REVIEW', path, 'Source review pending: ' + str(meta.get('extraction_status')))
        if type_ == 'evidence':
            if not meta.get('locator') or not meta.get('assertion_type'):
                issue('ERROR', 'EVIDENCE_METADATA', path, 'Missing locator or assertion_type')
            if state != 'verified':
                issue('WARN', 'EVIDENCE_REVIEW', path, 'Evidence needs source review; verified does not mean business approval')
            if meta.get('repo_id'):
                try:
                    _, rm, _ = by_id(root, 'REPO-' + meta['repo_id'])
                    current = contained(repo_location(root, rm), meta['source_path'])
                    if digest(current.read_bytes()) != meta.get('source_sha256'):
                        issue('ERROR' if state == 'verified' else 'WARN', 'STALE_EVIDENCE', path, 'Referenced repository file changed; review evidence again')
                    snap = contained(root, '03_Systems/Snapshots/' + meta['snapshot_id'])
                    manifest = json.loads((snap / 'manifest.json').read_text())
                    if manifest['files'].get(meta['source_path']) != meta.get('source_sha256'):
                        issue('ERROR', 'SNAPSHOT_MISMATCH', path, 'Evidence and snapshot versions differ')
                    if manifest.get('openwiki_active_run'):
                        issue('WARN', 'OPENWIKI_INCOMPLETE', path, 'OpenWiki had an unfinished run when captured')
                except (OSError, ValueError, KeyError) as exc:
                    issue('ERROR', 'EVIDENCE_SOURCE', path, str(exc))
            elif meta.get('source_id'):
                try:
                    _, sm, _ = by_id(root, meta['source_id'])
                    if sm.get('sha256') != meta.get('source_sha256'):
                        issue('ERROR', 'STALE_EVIDENCE', path, 'Source version differs from evidence')
                    if state == 'verified' and sm.get('review_status') != 'verified':
                        issue('ERROR', 'VISUAL_REVIEW_REQUIRED', path, 'Original material review is incomplete')
                except (ValueError, OSError) as exc:
                    issue('ERROR', 'EVIDENCE_SOURCE', path, str(exc))
            else:
                issue('ERROR', 'EVIDENCE_SOURCE', path, 'Evidence must identify material or repository provenance')
    profile = okf.inspect_authored(root)
    for message in profile['errors']: issue('ERROR','OKF_PROFILE',root/'90_Agent/OKF-Profile.md',message)
    for message in profile['warnings']: issue('WARN','OKF_GUIDANCE',root/'90_Agent/OKF-Profile.md',message)
    findings.extend(baselines.inspect(root, records))
    return findings


def write_report(root, strict=False):
    findings = inspect(root)
    target = root / '90_Agent/Reports/Quality-Report.md'
    errors = sum(x['level'] == 'ERROR' for x in findings)
    warnings = sum(x['level'] == 'WARN' for x in findings)
    body = f'# Document quality report\n\nErrors: {errors}; warnings: {warnings} .\n\n'
    body += 'Checks cover structure, traceability, source hashes and terminology hints. Semantics, completeness, estimates and business decisions still need review.\n\n'
    for item in findings:
        body += f'- **{item["level"]} {item["code"]}** {link(root, root/item["file"])}：{item["message"]}\n'
    if not findings:
        body += 'No detectable issues.\n'
    save_note(target, {'id': 'REPORT-QUALITY', 'type': 'report', 'language': 'en', 'generated_at': stamp()}, body)
    return target, errors + (warnings if strict else 0)
