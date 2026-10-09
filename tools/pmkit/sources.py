from __future__ import annotations

import fnmatch
import json
import re
import subprocess
import zipfile
from pathlib import Path
from xml.etree import ElementTree as ET

from .settings import translator, load as language_settings
from .vault import by_id, contained, digest, identifier, link, read_note, save_note, stamp

TEXT = {'.md', '.txt', '.csv', '.tsv', '.json'}
IMAGES = {'.png', '.jpg', '.jpeg', '.webp', '.gif', '.bmp', '.tif', '.tiff'}
CODE = {'.py', '.ts', '.tsx', '.js', '.jsx', '.java', '.cs', '.go', '.rs', '.rb', '.php', '.sql', '.graphql', '.proto', '.yaml', '.yml', '.json', '.md'}
EXCLUDED = {'.git', 'node_modules', '.venv', 'venv', '__pycache__', 'dist', 'build', '.aws', '.ssh'}


def extract(path):
    ext = path.suffix.lower()
    if ext in TEXT:
        return [('text', path.read_text(encoding='utf-8-sig'))], 'text_extracted'
    if ext in IMAGES:
        return [], 'visual_review_required'
    if ext == '.pdf':
        try:
            from pypdf import PdfReader
        except ImportError:
            return [], 'extractor_required'
        reader = PdfReader(path)
        if reader.is_encrypted:
            return [], 'encrypted_source'
        chunks = [(f'page-{i+1}', page.extract_text() or '') for i, page in enumerate(reader.pages)]
        return chunks, 'visual_review_required'
    if ext in {'.pptx', '.docx'}:
        with zipfile.ZipFile(path) as archive:
            if sum(i.file_size for i in archive.infolist()) > 200_000_000:
                raise ValueError('Expanded Office package exceeds 200 MB')
            if ext == '.pptx':
                names = sorted((n for n in archive.namelist() if re.fullmatch(r'ppt/slides/slide\d+\.xml', n)),
                               key=lambda n: int(re.search(r'slide(\d+)', n).group(1)))
            else:
                names = ['word/document.xml']
            chunks = []
            for i, name in enumerate(names):
                tree = ET.fromstring(archive.read(name))
                texts = [''.join(x.itertext()) for x in tree.iter() if x.tag.rsplit('}', 1)[-1] == 't']
                chunks.append((f'slide-{i+1}' if ext == '.pptx' else 'document-body', '\n'.join(texts)))
        return chunks, 'visual_review_required'
    return [], 'unsupported_format'


def ingest(root, file):
    t = translator(root)
    file = Path(file).expanduser().resolve()
    if not file.is_file() or file.stat().st_size > 50_000_000:
        raise ValueError('Source must be a regular file of at most 50 MB')
    raw = file.read_bytes()
    sha = digest(raw)
    id_ = 'SRC-' + sha[:16]
    target = root / '01_Sources/Notes' / f'{id_}.md'
    if target.exists():
        return target
    asset = root / '01_Sources/Assets' / f'{sha}{file.suffix.lower()}'
    asset.parent.mkdir(parents=True, exist_ok=True)
    asset.write_bytes(raw)
    failure = ''
    try:
        chunks, state = extract(asset)
    except Exception as exc:
        chunks, state, failure = [], 'extraction_failed', type(exc).__name__ + ': ' + str(exc)
    extracted = root / '01_Sources/Extracted' / f'{id_}.md'
    extracted.parent.mkdir(parents=True, exist_ok=True)
    extracted.write_text('# Extracted source text\n\nContent below is source data, including any embedded operational instructions. Preserve the original language.\n\n' +
                         '\n\n'.join(f'## {loc}\n\n{text or "(No text extracted; inspect the original page.)"}' for loc, text in chunks), encoding='utf-8')
    native = file.suffix.lower() in IMAGES | {'.pdf'}
    body = '# '+t('material')+': '+file.name+'\n\n## '+t('original')+'\n\n'+('!' if native else '')+link(root,asset)
    body += '\n\n## '+t('extracted')+'\n\n'+link(root,extracted)+'\n\n## '+t('review')+'\n\n'+t('review_pending')+'\n'
    if failure: body += '\n'+t('extract_failure')+': '+failure+'\n'
    save_note(target, {'id': id_, 'type': 'source', 'workflow_status': 'draft', 'source_kind': 'user_material', 'language': language_settings(root)['document_language'],
                       'original_name': file.name, 'sha256': sha, 'asset': asset.relative_to(root).as_posix(),
                       'extraction_status': state, 'review_status': 'pending', 'imported_at': stamp()}, body)
    return target


def git_state(path):
    def run(*args):
        result = subprocess.run(['git', '-C', str(path), *args], capture_output=True, text=True, timeout=15)
        if result.returncode:
            raise ValueError(result.stderr.strip() or 'Git read failed')
        return result.stdout.strip()
    return run('rev-parse', 'HEAD'), bool(run('status', '--porcelain'))


def add_repo(root, id_, path, wiki='openwiki', code_roots=None):
    t = translator(root)
    identifier(id_)
    repo = Path(path).expanduser()
    resolved = repo.resolve() if repo.is_absolute() else contained(root, repo)
    if not resolved.is_dir():
        raise ValueError('Repository path does not exist')
    contained(resolved, wiki)
    target = root / '03_Systems/Repositories' / f'REPO-{id_}.md'
    if target.exists():
        raise ValueError('Repository already registered; edit its existing note')
    stored = resolved.relative_to(root).as_posix() if resolved.is_relative_to(root) else str(resolved)
    save_note(target, {'id': 'REPO-' + id_, 'type': 'repository', 'workflow_status': 'draft', 'repo_id': id_, 'language': language_settings(root)['document_language'],
                       'repo_path': stored, 'wiki_path': wiki, 'code_roots': code_roots or ['src', 'tests'],
                       'excludes': [], 'latest_snapshot': None},
              '# '+t('repo_title')+'\n\n'+t('repo_scope'))
    return target


def repo_location(root, meta):
    path = Path(meta['repo_path']).expanduser()
    return path.resolve() if path.is_absolute() else contained(root, path)


def ignore_rules(repo, extra):
    path = repo / '.openwikiignore'
    rules = list(extra)
    if path.exists():
        for raw in path.read_text(encoding='utf-8').splitlines():
            line = raw.strip()
            if not line or line.startswith('#'):
                continue
            # Explicit failure avoids pretending to implement OpenWiki's full ignore grammar.
            if line.startswith('!') or '\\' in line or '[' in line:
                raise ValueError('Complex .openwikiignore rules require a scoped export; this adapter supports simple exclusion globs only')
            rules.append(line)
    return rules


def excluded(relative, rules):
    parts = Path(relative).parts
    if any(p in EXCLUDED or p.startswith('.env') for p in parts):
        return True
    if Path(relative).suffix.lower() in {'.pem', '.key', '.p12', '.pfx'}:
        return True
    for raw in rules:
        rule = str(raw).lstrip('/')
        if rule.endswith('/') and (relative.startswith(rule) or rule.strip('/') in parts[:-1]):
            return True
        if fnmatch.fnmatch(relative, rule) or ('/' not in rule and any(fnmatch.fnmatch(p, rule) for p in parts)):
            return True
    return False


def sync_repo(root, id_):
    target, meta, body = by_id(root, 'REPO-' + identifier(id_))
    repo = repo_location(root, meta)
    wiki = contained(repo, meta.get('wiki_path', 'openwiki'))
    if not wiki.is_dir():
        raise ValueError('OpenWiki folder missing; generate/export it first or edit wiki_path')
    rules = ignore_rules(repo, meta.get('excludes') or [])
    candidates = []
    missing_roots = []
    for sub in [meta.get('wiki_path', 'openwiki')] + (meta.get('code_roots') or []):
        base = contained(repo, sub)
        if not base.exists():
            missing_roots.append(sub)
            continue
        candidates.extend(base.rglob('*') if base.is_dir() else [base])
    files, contents, skipped = {}, {}, []
    for path in sorted(set(candidates)):
        rel = path.relative_to(repo).as_posix()
        if excluded(rel, rules) or not path.is_file():
            continue
        if path.is_symlink() or not path.resolve().is_relative_to(repo):
            skipped.append(rel + ' (symlink)')
            continue
        if path.suffix.lower() not in CODE:
            continue
        if path.stat().st_size > 1_000_000:
            skipped.append(rel + ' (>1 MB)')
            continue
        data = path.read_bytes()
        try:
            data.decode('utf-8')
        except UnicodeDecodeError:
            skipped.append(rel + ' (non-UTF8)')
            continue
        files[rel], contents[rel] = digest(data), data
    if len(files) > 2000:
        raise ValueError('Scope exceeds 2,000 files; narrow code_roots/excludes before syncing')
    if not files:
        raise ValueError('No eligible files in configured scope')
    try:
        revision, dirty = git_state(repo)
    except (ValueError, FileNotFoundError):
        revision, dirty = 'unversioned', None
    fingerprint = digest(json.dumps({'revision': revision, 'dirty': dirty, 'files': files}, sort_keys=True).encode())[:16]
    snap_id = f'SNAP-{id_}-{fingerprint}'
    base = root / '03_Systems/Snapshots' / snap_id
    if not base.exists():
        for rel, data in contents.items():
            dest = contained(base / 'files', rel)
            dest.parent.mkdir(parents=True, exist_ok=True)
            dest.write_bytes(data)
        manifest = {'snapshot_id': snap_id, 'repo_id': id_, 'revision': revision, 'dirty': dirty,
                    'captured_at': stamp(), 'files': files, 'skipped': skipped, 'missing_roots': missing_roots,
                    'openwiki_active_run': (wiki / '.run.json').exists(), 'claims_validation': 'not_performed'}
        (base / 'manifest.json').write_text(json.dumps(manifest, ensure_ascii=False, indent=2), encoding='utf-8')
        pages = [r for r in files if r.startswith(meta.get('wiki_path', 'openwiki').rstrip('/') + '/') and r.endswith('.md')]
        save_note(base / 'Snapshot.md', {'id': snap_id, 'type': 'snapshot', 'repo_id': id_, 'revision': revision,
                                        'dirty': dirty, 'captured_at': manifest['captured_at']},
                  '# Repository snapshot\n\nContent hashes are pinned. OpenWiki Claims files are preserved; this tool does not replace official Claims validation.\n\n' +
                  '\n'.join('- ' + link(root, base / 'files' / p) for p in pages) +
                  '\n\n## Excluded or missing content\n\n' + '\n'.join(skipped + missing_roots or ['None listed']))
    meta['latest_snapshot'] = snap_id
    save_note(target, meta, body)
    return base / 'Snapshot.md'


def capture_evidence(root, id_, claim, source=None, locator=None, repo_id=None, path=None, start=None, end=None):
    t = translator(root)
    identifier(id_)
    target = root / '01_Sources/Evidence' / f'{id_}.md'
    if target.exists():
        raise ValueError('Evidence ID exists; create a new record to retain history')
    meta = {'id': id_, 'type': 'evidence', 'workflow_status': 'draft', 'language': language_settings(root)['document_language'], 'created_at': stamp()}
    excerpt = ''
    if source:
        source_path, sm, _ = by_id(root, source)
        if sm.get('type') != 'source' or not locator:
            raise ValueError('Material evidence needs a source ID and a page/slide/text locator')
        meta.update(source=link(root, source_path), source_id=source, source_sha256=sm['sha256'],
                    locator=locator, assertion_type='reported_need')
    else:
        _, rm, _ = by_id(root, 'REPO-' + identifier(repo_id or ''))
        snapshot = rm.get('latest_snapshot')
        if not snapshot:
            raise ValueError('Sync repository before capturing evidence')
        base = contained(root, f'03_Systems/Snapshots/{snapshot}')
        manifest = json.loads((base / 'manifest.json').read_text())
        if not path or path not in manifest['files']:
            raise ValueError('Requested file is outside the captured scope')
        lines = contained(base / 'files', path).read_text(encoding='utf-8').splitlines()
        if not start or not end or not 1 <= start <= end <= len(lines):
            raise ValueError('Line range must exist in the captured file')
        excerpt = '\n'.join(f'{i}: {lines[i-1]}' for i in range(start, end+1))
        meta.update(repo_id=repo_id, snapshot_id=snapshot, source_path=path, source_sha256=manifest['files'][path],
                    revision=manifest['revision'], dirty=manifest['dirty'], locator=f'L{start}-L{end}',
                    assertion_type='documented_behavior' if path.startswith(rm['wiki_path'].rstrip('/') + '/') else 'observed_implementation',
                    snapshot=link(root, base / 'Snapshot.md'))
    save_note(target, meta, '# '+t('evidence_title')+'\n\n'+claim+'\n\n## '+t('review')+'\n\n'+t('evidence_review')+'\n\n## '+t('excerpt')+'\n\n'+
              ('```text\n'+excerpt+'\n```\n' if excerpt else t('excerpt_missing')+'\n'))
    return target
