from __future__ import annotations

import json
import re
from pathlib import Path

from .vault import by_id, catalog, contained, content_hash, digest, identifier, link, read_note, save_note, stamp

DOCUMENT_TYPES = {'brd', 'prd', 'requirement', 'glossary', 'design', 'contract'}


def capture(root, baseline_id, includes):
    """Capture a candidate. This operation neither approves nor changes source documents."""
    identifier(baseline_id)
    if not includes or len(set(includes)) != len(includes):
        raise ValueError('Baseline needs a nonempty, unique ID list')
    folder = root / '00_Governance/Baselines' / baseline_id
    if folder.exists():
        raise ValueError('Baseline ID already exists; capture a new ID')
    entries, payloads = {}, {}
    for id_ in includes:
        path, meta, body = by_id(root, id_)
        if meta.get('type') not in DOCUMENT_TYPES:
            raise ValueError('Baseline supports controlled documents only: ' + id_)
        if type(meta.get('version')) is not int or meta['version'] < 1:
            raise ValueError('Positive integer document version required: ' + id_)
        identifier(id_)
        data = path.read_bytes()
        entries[id_] = {'path': path.relative_to(root).as_posix(), 'version': meta['version'],
                        'content_sha256': content_hash(meta, body), 'file_sha256': digest(data),
                        'snapshot': 'files/' + id_ + '.md'}
        payloads[id_] = data
    manifest = {'schema_version': 2, 'id': baseline_id, 'created_at': stamp(), 'documents': entries}
    folder.mkdir(parents=True)
    (folder / 'files').mkdir()
    for id_, data in payloads.items():
        (folder / entries[id_]['snapshot']).write_bytes(data)
    (folder / 'manifest.json').write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + '\n')
    return folder / 'manifest.json'


def load(root, baseline_id):
    identifier(baseline_id)
    folder = root / '00_Governance/Baselines' / baseline_id
    data = (folder / 'manifest.json').read_bytes()
    manifest = json.loads(data)
    if manifest.get('id') != baseline_id or manifest.get('schema_version') != 2:
        raise ValueError('Invalid baseline manifest')
    if not manifest.get('documents'):
        raise ValueError('Empty baseline')
    for id_, entry in manifest['documents'].items():
        snapshot = contained(folder, entry['snapshot'])
        if digest(snapshot.read_bytes()) != entry['file_sha256']:
            raise ValueError('Baseline snapshot modified: ' + id_)
        meta, body = read_note(snapshot)
        if meta.get('id') != id_ or meta.get('version') != entry['version'] or content_hash(meta, body) != entry['content_sha256']:
            raise ValueError('Baseline content does not match manifest: ' + id_)
    return manifest, digest(data)


def approval(root, baseline_id):
    manifest, sha = load(root, baseline_id)
    matching = [(p,m,b) for p,m,b in catalog(root) if m.get('type')=='decision' and m.get('decision_type')=='baseline_approval' and m.get('baseline_id')==baseline_id]
    if any(m.get('workflow_status')=='rescinded' and not m.get('simulation') for _,m,_ in matching):
        raise ValueError('Approval has been rescinded; capture a new baseline if a new decision is needed')
    # The decision is a project record, not a cryptographic identity or access-control system.
    for path, meta, body in matching:
        if not meta.get('simulation') and meta.get('workflow_status') == 'accepted' and meta.get('baseline_sha256') == sha and all(meta.get(k) for k in ['decided_by', 'decided_at', 'decision_evidence']):
            return manifest, path
    raise ValueError('No accepted decision for this exact baseline manifest: ' + baseline_id)


def pinned_document(root, baseline_id, id_):
    manifest, _ = approval(root, baseline_id)
    if id_ not in manifest['documents']:
        raise ValueError('Document is not in the approved baseline: ' + id_)
    entry = manifest['documents'][id_]
    return read_note(contained(root / '00_Governance/Baselines' / baseline_id, entry['snapshot']))


def inspect(root, records):
    findings = []
    def add(level, code, path, message):
        findings.append({'level': level, 'code': code, 'file': path.relative_to(root).as_posix(), 'message': message})
    for path, meta, body in records:
        if meta.get('type') not in DOCUMENT_TYPES or meta.get('doc_status') != 'approved':
            continue
        try:
            baseline_id = meta.get('baseline_id')
            if not baseline_id:
                raise ValueError('Approved document needs baseline_id')
            manifest, _ = approval(root, baseline_id)
            entry = manifest['documents'].get(meta['id'])
            if not entry or entry['version'] != meta.get('version') or entry['content_sha256'] != content_hash(meta, body):
                raise ValueError('Current text differs from its approved version; create a new draft/version')
        except (ValueError, OSError, KeyError, TypeError) as exc:
            add('ERROR', 'BASELINE_INVALID', path, str(exc))
    base = root / '00_Governance/Baselines'
    for file in sorted(base.glob('*/manifest.json')):
        try:
            load(root, file.parent.name)
        except (ValueError, OSError, KeyError, TypeError) as exc:
            add('ERROR', 'BASELINE_INTEGRITY', file, str(exc))
    return findings


def impact(root, id_):
    source = by_id(root, id_)
    records = catalog(root)
    bypath = {p.resolve(): m['id'] for p, m, _ in records}
    contexts = {m.get('context'): m['id'] for _, m, _ in records if m.get('type') == 'glossary'}
    from .vault import resolve_link
    reverse = {}
    for path, meta, body in records:
        if meta.get('type') == 'report':
            continue
        refs = set()
        for raw in re.findall(r'\[\[([^\]]+)\]\]', json.dumps(meta, ensure_ascii=False) + '\n' + body):
            try:
                ref = bypath.get(resolve_link(root, raw).resolve())
                if ref: refs.add(ref)
            except (ValueError, OSError):
                pass
        refs.update(contexts[c] for c in meta.get('contexts', []) if c in contexts)
        for ref in refs:
            reverse.setdefault(ref, set()).add(meta['id'])
    reached, queue = set(), [id_]
    while queue:
        for downstream in reverse.get(queue.pop(0), set()):
            if downstream != id_ and downstream not in reached:
                reached.add(downstream); queue.append(downstream)
    target = root / '90_Agent/Reports' / ('Impact-' + identifier(id_) + '.md')
    body = '# Change impact candidates\n\nStarting point: ' + link(root, source[0]) + '\n\nExpanded through document and context references. Matches are review candidates, not proof of invalidity. Historical baselines preserve their pinned versions.\n\n'
    body += '\n'.join('- ' + link(root, p) + ' (' + m.get('type', '') + ')' for p, m, _ in records if m['id'] in reached) or 'No downstream references found.'
    save_note(target, {'id': 'REPORT-IMPACT-' + id_, 'type': 'report', 'language': 'en', 'generated_at': stamp()}, body)
    return target
