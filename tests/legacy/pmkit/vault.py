from __future__ import annotations

import hashlib
import json
import re
from datetime import datetime
from pathlib import Path
from zoneinfo import ZoneInfo


def stamp():
    return datetime.now(ZoneInfo('Asia/Taipei')).isoformat(timespec='seconds')


def digest(data):
    return hashlib.sha256(data).hexdigest()


def scalar(text):
    text = text.strip()
    if not text:
        return None
    if text.startswith('"') or text.startswith('[') or text.startswith('{'):
        try:
            return json.loads(text)
        except json.JSONDecodeError as exc:
            raise ValueError('Use JSON-style quotes for inline arrays and quoted properties') from exc
    if text.startswith("'") and text.endswith("'"):
        return text[1:-1].replace("''", "'")
    text = re.split(r'\s+#', text, maxsplit=1)[0].rstrip()
    if text in {'true', 'false'}:
        return text == 'true'
    if text in {'null', '~'}:
        return None
    if text in {'|', '>', '{}'} or text.startswith('{'):
        raise ValueError('Nested/multiline properties are unsupported; put structured content in the note body')
    if re.fullmatch(r'-?\d+(?:\.\d+)?', text):
        return float(text) if '.' in text else int(text)
    return text


def read_note(path):
    """Read Obsidian flat properties, block lists, or JSON frontmatter; fail on ambiguity."""
    text = Path(path).read_text(encoding='utf-8-sig')
    if not text.startswith('---\n'):
        return {}, text
    parts = text.split('\n---', 1)
    if len(parts) != 2:
        raise ValueError(f'Unclosed properties: {path}')
    header, body = parts[0][4:], parts[1].lstrip('\r\n')
    if header.lstrip().startswith('{'):
        meta = json.loads(header)
        if not isinstance(meta, dict):
            raise ValueError('Properties must be an object')
        return meta, body
    meta, last = {}, None
    for line in header.splitlines():
        if not line.strip() or line.lstrip().startswith('#'):
            continue
        match = re.match(r'^\s*-\s+(.*)$', line)
        if match:
            if last is None or meta[last] not in (None, []) and not isinstance(meta[last], list):
                raise ValueError(f'Invalid property list: {path}')
            if meta[last] is None:
                meta[last] = []
            meta[last].append(scalar(match.group(1)))
            continue
        match = re.match(r'^([A-Za-z_][\w-]*):(?:\s+(.*)|\s*)$', line)
        if not match:
            raise ValueError(f'Unsupported property syntax in {path}: {line}')
        key = match.group(1)
        if key in meta:
            raise ValueError(f'Duplicate property {key}: {path}')
        meta[key] = scalar(match.group(2) or '')
        last = key
    return meta, body


def save_note(path, meta, body):
    meta = dict(meta)
    if not meta.get('type'): raise ValueError('Generated notes require a nonempty type')
    title = re.search(r'^#\s+(.+)$', body, re.M)
    meta.setdefault('title', title.group(1) if title else Path(path).stem)
    lines = ['---']
    for key, value in meta.items():
        lines.append(f'{key}: {json.dumps(value, ensure_ascii=False)}')
    lines.extend(['---', '', body.rstrip(), ''])
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text('\n'.join(lines), encoding='utf-8')


def contained(root, relative):
    root = Path(root).resolve()
    path = (root / relative).resolve()
    if not path.is_relative_to(root):
        raise ValueError(f'Path escapes configured root: {relative}')
    return path


def identifier(value):
    if not re.fullmatch(r'[A-Za-z0-9][A-Za-z0-9_-]{0,79}', value):
        raise ValueError('IDs must contain only letters, numbers, underscores and hyphens')
    return value


def link(root, path):
    rel = Path(path).relative_to(root).as_posix()
    if rel.endswith('.md'):
        rel = rel[:-3]
    return f'[[{rel}]]'


def managed_notes(root):
    for folder in ['00_Governance', '01_Sources/Notes', '01_Sources/Evidence', '02_Domain',
                   '03_Systems/Repositories', '03_Systems/Analysis', '04_Requirements',
                   '05_Backlog', '06_Delivery', '07_Decisions', '08_Verification', '09_Operations']:
        base = root / folder
        if base.exists():
            yield from (p for p in sorted(base.rglob('*.md')) if 'Baselines' not in p.relative_to(root).parts)


def catalog(root):
    records = []
    for path in managed_notes(root):
        meta, body = read_note(path)
        if meta.get('id'):
            records.append((path, meta, body))
    return records


def by_id(root, id_):
    found = [r for r in catalog(root) if r[1]['id'] == id_]
    if len(found) != 1:
        raise ValueError(f'Expected exactly one note with ID {id_}, got {len(found)}')
    return found[0]


def content_hash(meta, body):
    """Pin substantive content; workflow annotations do not change the approved text."""
    annotations = {'doc_status', 'baseline_id', 'reviewed_by', 'reviewed_at', 'decision'}
    payload = {'properties': {k: v for k, v in meta.items() if k not in annotations},
               'body': body.strip()}
    return digest(json.dumps(payload, sort_keys=True, ensure_ascii=False).encode())


def resolve_link(root, value):
    text = str(value).removeprefix('[[').removesuffix(']]').split('|', 1)[0].split('#', 1)[0]
    direct = contained(root, text)
    if direct.is_file():
        return direct
    markdown = Path(str(direct) + '.md')
    if direct.suffix != '.md' and markdown.is_file():
        return markdown
    matches = list(root.rglob(text + '.md')) if '/' not in text else []
    if len(matches) == 1:
        return matches[0]
    raise ValueError(f'Unresolved or ambiguous link: {value}')


def tables(body):
    rows, header = [], None
    for line in body.splitlines() + ['']:
        if not line.strip().startswith('|'):
            if header:
                yield header, rows
            header, rows = None, []
            continue
        cells = [c.strip() for c in line.strip().strip('|').split('|')]
        if all(re.fullmatch(r'[:\- ]+', c) for c in cells):
            continue
        if header is None:
            header = cells
        elif len(cells) == len(header):
            rows.append(dict(zip(header, cells)))
        else:
            raise ValueError('Table row has a different number of columns than its header')


def table_with(body, headers):
    found = [rows for cols, rows in tables(body) if cols == headers]
    if len(found) != 1:
        raise ValueError(f'Expected one table with columns: {headers}')
    return found[0]
