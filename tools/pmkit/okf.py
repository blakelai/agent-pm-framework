"""OKF producer profile and portable export. This is not a Google-certified validator."""
from __future__ import annotations

import json
import os
import re
import shutil
import zipfile
from pathlib import Path
from urllib.parse import quote, unquote, urlsplit

from .vault import read_note, save_note, resolve_link, stamp

SPEC = 'https://github.com/GoogleCloudPlatform/knowledge-catalog/blob/main/okf/SPEC.md'
RAW_PREFIXES = ('01_Sources/Assets/', '01_Sources/Extracted/',
                '03_Systems/Snapshots/', '00_Governance/Baselines/')


def is_raw(relative):
    return Path(relative).as_posix().startswith(RAW_PREFIXES)


def authored_notes(root):
    for path in sorted(root.rglob('*.md')):
        rel = path.relative_to(root)
        if is_raw(rel) or any(p in {'.agents','.git','.obsidian','__pycache__','exports'} for p in rel.parts):
            continue
        yield path


def parse_yaml(path):
    try:
        import yaml
    except ImportError as exc:
        raise ValueError('OKF validation requires PyYAML; install requirements.txt') from exc
    class UniqueLoader(yaml.SafeLoader):
        pass
    def unique_mapping(loader, node, deep=False):
        result={}
        for key_node,value_node in node.value:
            key=loader.construct_object(key_node,deep=deep)
            if key in result: raise ValueError('Duplicate frontmatter key: '+str(key))
            result[key]=loader.construct_object(value_node,deep=deep)
        return result
    UniqueLoader.add_constructor(yaml.resolver.BaseResolver.DEFAULT_MAPPING_TAG,unique_mapping)
    text=path.read_text(encoding='utf-8')
    match=re.match(r'\A---\r?\n(.*?)\r?\n---(?:\r?\n|\Z)',text,re.S)
    if not match: return None,text
    try: meta=yaml.load(match.group(1),Loader=UniqueLoader)
    except (yaml.YAMLError,TypeError) as exc: raise ValueError('Invalid YAML frontmatter: '+str(exc)) from exc
    if not isinstance(meta,dict): raise ValueError('Frontmatter must be a mapping')
    return meta,text[match.end():]


def check_paths(root, paths, strict_profile=False):
    errors=[]; warnings=[]; count=0
    for path in paths:
        count+=1; rel=path.relative_to(root).as_posix()
        try:
            meta,body=parse_yaml(path)
            if path.name=='index.md':
                if meta is not None and (path.parent!=root or set(meta)-{'okf_version'}):
                    raise ValueError('Only root index.md may have frontmatter, containing okf_version')
                if not re.search(r'^#{1,6} .+',body,re.M): raise ValueError('Index needs a section heading')
                if not re.search(r'^[-*] \[[^\]]+\]\([^)]+\)',body,re.M): raise ValueError('Index needs Markdown list links')
            elif path.name=='log.md':
                if meta is not None: raise ValueError('log.md must not carry concept frontmatter')
                for heading in re.findall(r'^## (.+)$',body,re.M):
                    from datetime import date
                    date.fromisoformat(heading)
                if not re.search(r'^## \d{4}-\d{2}-\d{2}$',body,re.M): raise ValueError('Log needs date-grouped entries')
            else:
                if not meta or not isinstance(meta.get('type'),str) or not meta['type'].strip():
                    raise ValueError('Concept needs YAML frontmatter with a nonempty string type')
                if strict_profile and not isinstance(meta.get('title'),str):
                    raise ValueError('This project profile requires a title')
                if 'status' in meta and meta['status'] not in {'draft','stable','deprecated'}:
                    warnings.append(rel+': use workflow_status for project states; status is the OKF lifecycle')
                if 'sources' in meta and (not isinstance(meta['sources'],list) or any(not isinstance(s,dict) or not s.get('resource') for s in meta['sources'])):
                    warnings.append(rel+': optional sources entries should contain resource')
                # Optional fields are guidance, not extra core conformance gates.
        except (ValueError,OSError,UnicodeError) as exc: errors.append(rel+': '+str(exc))
    return {'okf_version':'0.2','documents_checked':count,'errors':errors,'warnings':warnings,
            'conformant':bool(count) and not errors,'scope':'project profile' if strict_profile else 'bundle structure'}


def inspect_authored(root):
    return check_paths(root,authored_notes(root),strict_profile=True)


def check_bundle(root):
    return check_paths(root,sorted(root.rglob('*.md')))


def outside_fences(body, transform):
    chunks=re.split(r'(```.*?```|~~~.*?~~~)',body,flags=re.S)
    return ''.join(chunk if i%2 else transform(chunk) for i,chunk in enumerate(chunks))


def export(root, output):
    """Write a fresh, read-only knowledge bundle. Never rewrite source or baseline bytes."""
    root=root.resolve(); output=output.expanduser().resolve()
    if output==root or output.is_relative_to(root) or root.is_relative_to(output):
        raise ValueError('Export to a separate directory outside the working Vault')
    if output.exists(): raise ValueError('Export destination exists; use a new directory to preserve history')
    authored=list(authored_notes(root))
    check=inspect_authored(root)
    if check['errors']: raise ValueError('Fix the authored document profile before export: '+str(check['errors']))
    skills=sorted((root/'.agents/skills').glob('*/SKILL.md'))
    mapping={p.relative_to(root).as_posix():p.relative_to(root).as_posix() for p in authored}
    mapping.update({p.relative_to(root).as_posix():'90_Agent/Skills/'+p.parent.name+'.md' for p in skills})
    output.mkdir(parents=True)
    # Preserve raw/source Markdown as exact-byte attachments, not rewritten concepts.
    for p in sorted(root.rglob('*')):
        if not p.is_file(): continue
        rel=p.relative_to(root); text=rel.as_posix()
        if any(x in {'.git','.obsidian','__pycache__','exports'} for x in rel.parts) or p.suffix=='.pyc':continue
        if text in mapping:continue
        if text.startswith('.agents/'):continue
        dest_rel=text+'.txt' if p.suffix=='.md' else text
        mapping[text]=dest_rel
        dest=output/dest_rel;dest.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(p,dest)

    def resolve(original, target):
        target=unquote(target)
        if urlsplit(target).scheme or target.startswith('#'): return target
        filename,sep,anchor=target.partition('#')
        source=(root/filename.lstrip('/')) if filename.startswith('/') else (original.parent/filename)
        try: rel=source.resolve().relative_to(root).as_posix()
        except ValueError:return target
        if rel not in mapping:return target
        destination=output/mapping[rel]
        origin=output/mapping[original.relative_to(root).as_posix()]
        path=quote(os.path.relpath(destination,origin.parent).replace(os.sep,'/'),safe='/._-')
        return path+('#'+anchor.lstrip('^') if sep else '')

    for original in authored+skills:
        rel=original.relative_to(root).as_posix();meta,body=read_note(original)
        if original in skills:
            meta={'type':'Agent Skill','title':meta['name'],'description':meta['description'],'language':'en','skill_name':meta['name']}
            body='> Knowledge representation of a Skill. Use the working Vault for native Skill invocation.\n\n'+body
        def links(chunk):
            chunk=re.sub(r'(?<!!)\[([^\]]+)\]\(([^)]+)\)',lambda m:'['+m[1]+']('+resolve(original,m[2])+')',chunk)
            def wiki(match):
                raw=match[1];target_part=raw.split('|',1)[0];label=raw.split('|',1)[1] if '|' in raw else target_part.split('#',1)[0].rsplit('/',1)[-1]
                try: target=resolve_link(root,raw)
                except ValueError:return match[0]
                full='/'+target.relative_to(root).as_posix()
                if '#' in target_part:full+='#'+target_part.split('#',1)[1]
                return '['+label+']('+resolve(original,full)+')'
            chunk=re.sub(r'!?\[\[([^\]]+)\]\]',wiki,chunk)
            return re.sub(r'\^([A-Za-z0-9_-]+)\s*$',r'<a id="\1"></a>',chunk,flags=re.M)
        body=outside_fences(body,links)
        # Retain PM extensions while exposing their graph in ordinary Markdown.
        refs=[]
        for raw in re.findall(r'\[\[([^\]]+)\]\]',json.dumps(meta,ensure_ascii=False)):
            try:
                target=resolve_link(root,raw);href=resolve(original,'/'+target.relative_to(root).as_posix())
                refs.append('['+target.stem+']('+href+')')
            except ValueError:pass
        if refs:body+='\n\n## Related records\n\n'+'\n'.join('- '+s for s in sorted(set(refs)))+'\n'
        if original.name in {'index.md','log.md'}:
            (output/mapping[rel]).parent.mkdir(parents=True,exist_ok=True)
            (output/mapping[rel]).write_text(original.read_text())
        else:save_note(output/mapping[rel],meta,body)
    # Index text is navigation metadata and remains English.
    index='---\nokf_version: "0.2"\n---\n\n# Project knowledge\n\n'
    for original in authored+skills:
        rel=original.relative_to(root).as_posix()
        if original.name in {'index.md','log.md'}:continue
        meta,_=read_note(original);label=meta.get('title',meta.get('name',original.stem))
        index+='- ['+label+']('+quote(mapping[rel],safe='/._-')+') - '+str(meta.get('description',meta.get('type','Agent Skill')))+'\n'
    (output/'index.md').write_text(index)
    (output/'export-map.json').write_text(json.dumps({'created_at':stamp(),'source_spec':SPEC,'paths':mapping,'note':'Raw .md attachments are exact bytes with a .txt suffix; native Skill adapters are exported as concepts.'},ensure_ascii=False,indent=2)+'\n')
    result=check_bundle(output)
    (output/'validation.json').write_text(json.dumps(result,ensure_ascii=False,indent=2)+'\n')
    if result['errors']:raise ValueError('Export failed structural validation: '+str(result['errors']))
    return result
