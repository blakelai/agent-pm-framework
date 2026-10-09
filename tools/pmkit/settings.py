"""Project language policy. Runtime instructions are always English."""
import json
import re
from pathlib import Path
from string import Formatter

from .vault import read_note, save_note

SETTINGS_PATH = '00_Governance/Settings.md'
AGENT_REFERENCES = {
    'AGENTS.md', 'README.md', 'HOME.md', 'References.md', 'index.md', 'log.md', SETTINGS_PATH,
    '00_Governance/Schema.md', '00_Governance/Workflow.md',
    '00_Governance/Project-Setup.md', '00_Governance/Language-Settings.md',
    '03_Systems/OpenWiki-Integration.md',
    '00_Governance/Baselines-and-Changes.md', '00_Governance/Discovery-and-Prioritization.md',
    '03_Systems/Integration-Review.md', '06_Delivery/Planning.md',
    '08_Verification/Acceptance-and-Release.md', '09_Operations/Benefits-and-Costs.md',
    'tools/README.md',
}


def language_tag(value):
    # A safe language-tag shape, not a complete IANA registry validator.
    if not isinstance(value, str) or not re.fullmatch(r'[A-Za-z]{2,8}(?:-[A-Za-z0-9]{1,8})*', value):
        raise ValueError('document_language must be a language tag such as zh-TW, en, or ja-JP')
    parts = value.split('-')
    return '-'.join([parts[0].lower()] + [p.title() if len(p)==4 and p.isalpha() else p.upper() if len(p)==2 and p.isalpha() else p.lower() for p in parts[1:]])


def load(root):
    meta, _ = read_note(root / SETTINGS_PATH)
    if meta.get('agent_language') != 'en':
        raise ValueError('agent_language is fixed to en for agent instructions and operational records')
    return {'document_language': language_tag(meta.get('document_language')), 'agent_language': 'en'}


def update(root, document_language):
    tag = language_tag(document_language)
    meta, body = read_note(root / SETTINGS_PATH)
    if meta.get('agent_language') != 'en':
        raise ValueError('Restore agent_language: en before changing project language')
    meta['document_language'] = tag
    save_note(root / SETTINGS_PATH, meta, body)
    return load(root)


def is_agent_document(relative_path):
    path = Path(relative_path).as_posix()
    return path in AGENT_REFERENCES or path.startswith(('.agents/', '90_Agent/', '80_Templates/'))


def required_language(root, relative_path):
    config = load(root)
    return 'en' if is_agent_document(relative_path) else config['document_language']


def catalog(root):
    tag = load(root)['document_language']
    folder = root / 'tools/locales'
    base = json.loads((folder / 'en.json').read_text())
    # English regional variants share the English renderer; Chinese variants remain explicit.
    renderer = 'en' if tag.split('-')[0] == 'en' else tag
    path = folder / (renderer + '.json')
    if not path.is_file():
        raise ValueError(f'No project-document renderer for {tag}. The agent must translate tools/locales/en.json into tools/locales/{tag}.json before generating project reports. English fallback is not published as a completed translation.')
    data = json.loads(path.read_text())
    if data.keys() != base.keys() or not all(isinstance(v,str) and v.strip() for v in data.values()):
        raise ValueError('Language catalog must contain exactly the English catalog keys with nonempty strings')
    def fields(value):
        return sorted((field, spec, conversion) for _,field,spec,conversion in Formatter().parse(value) if field is not None)
    for key, value in data.items():
        if fields(value) != fields(base[key]):
            raise ValueError('Language catalog changed formatting placeholders: ' + key)
    return data


def translator(root):
    data = catalog(root)
    return lambda key, **values: data[key].format(**values)
