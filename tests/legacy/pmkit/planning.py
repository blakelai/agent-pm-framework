from __future__ import annotations

import math
from datetime import date

from .settings import translator, load as language_settings
from .vault import catalog, link, read_note, resolve_link, save_note, stamp, table_with

STATES = {'proposed', 'ready', 'in_progress', 'blocked', 'done', 'cancelled'}


def number(value):
    value = float(value)
    if not math.isfinite(value):
        raise ValueError('Numbers must be finite')
    return value


def effort(body, remaining=False):
    header = ['Role', 'Remaining O', 'Remaining M', 'Remaining P'] if remaining else ['Role', 'O', 'M', 'P']
    result = {}
    for row in table_with(body, header):
        role = row['Role']
        if not role or role in result:
            raise ValueError('Missing or duplicate estimate role')
        o, m, p = [number(row[x]) for x in header[1:]]
        if not 0 <= o <= m <= p:
            raise ValueError('Require 0 <= O <= M <= P')
        result[role] = (o + 4*m + p) / 6
    if not result:
        raise ValueError('Empty estimate')
    return result


def calculate(root):
    t = translator(root)
    _, body = read_note(root / '06_Delivery/Iterations.md')
    periods = {}
    previous_end = None
    rows = table_with(body, ['Sprint', 'Start', 'End', 'Goal'])
    for row in sorted(rows, key=lambda r: date.fromisoformat(r['Start'])):
        s = int(row['Sprint']); start = date.fromisoformat(row['Start']); end = date.fromisoformat(row['End'])
        if s in periods or end < start or previous_end and start <= previous_end:
            raise ValueError('Duplicate, inverted or overlapping iterations')
        periods[s] = row; previous_end = end
    if not periods: raise ValueError('Planning is not configured: add periods to 06_Delivery/Iterations.md')
    config, _ = read_note(root / '06_Delivery/Planning.md')
    if not config.get('as_of'):
        raise ValueError('Planning is not configured: set as_of in 06_Delivery/Planning.md to YYYY-MM-DD')
    as_of = date.fromisoformat(config['as_of'])
    project, _ = read_note(root / '00_Governance/Project.md')
    _, people_body = read_note(root / '06_Delivery/People.md')
    caps, allocation, keys, calendars = {}, {}, set(), {}
    headers = ['Person', 'Role', 'Sprint', 'Project', 'FTE', 'Workdays', 'Absence', 'Focus', 'Reserve']
    for row in table_with(people_body, headers):
        s = int(row['Sprint']); person = row['Person']; role = row['Role']
        key = (person, s, row['Project'], role)
        if not person or not role or not row['Project'] or key in keys or s not in periods:
            raise ValueError('Invalid or duplicate person allocation')
        keys.add(key)
        fte, days, absence, focus, reserve = [number(row[x]) for x in headers[4:]]
        if not 0 <= fte <= 1 or days < 0 or not 0 <= absence <= fte*days or not 0 < focus <= 1 or not 0 <= reserve < 1:
            raise ValueError('Capacity inputs outside allowed range')
        window_start=max(date.fromisoformat(periods[s]['Start']),as_of)
        window_end=date.fromisoformat(periods[s]['End'])
        if days > max(0,(window_end-window_start).days+1):
            raise ValueError('Remaining workdays exceed remaining calendar days')
        person_period = person, s
        allocation[person_period] = allocation.get(person_period, 0) + fte
        if allocation[person_period] > 1 + 1e-9:
            raise ValueError(f'Person overallocated across roles/projects: {person}, Sprint {s}')
        if person_period in calendars and calendars[person_period] != days:
            raise ValueError('Conflicting remaining workdays for same person/Sprint')
        calendars[person_period] = days
        if row['Project'] != project['id']: continue
        data = caps.setdefault((s,role), {'capacity': 0., 'effort': 0., 'items': []})
        # Workdays and Absence describe time still available after as_of, not the original whole Sprint.
        if date.fromisoformat(periods[s]['End']) >= as_of:
            data['capacity'] += (fte*days-absence)*focus*(1-reserve)
    if not caps: raise ValueError('No allocations for current project')
    pbis = [r for r in catalog(root) if r[1].get('type') == 'pbi']
    ids = {m['id']: (p,m,b) for p,m,b in pbis}
    if len(ids) != len(pbis): raise ValueError('Duplicate PBI ID')
    graph, totals, unscheduled, messages, original = {}, {}, [], [], {}
    for path, meta, body in pbis:
        id_ = meta['id']; state = meta.get('delivery_status')
        if state not in STATES: raise ValueError('Invalid delivery_status: ' + id_)
        graph[id_] = []
        for ref in meta.get('depends_on', []):
            dm, _ = read_note(resolve_link(root, ref)); dep = dm.get('id')
            if dep not in ids: raise ValueError('PBI dependency must target PBI')
            graph[id_].append(dep)
        if state in {'done', 'cancelled'}:
            totals[id_] = 0.; continue
        sprint = meta.get('sprint')
        assigned = sprint is not None
        if assigned and (type(sprint) is not int or sprint not in periods):
            raise ValueError('Invalid Sprint: ' + id_)
        try:
            initial = effort(body)
            estimate = effort(body, remaining=True) if state in {'in_progress', 'blocked'} else initial
            original[id_] = sum(initial.values())
        except (ValueError, TypeError) as exc:
            if assigned: raise ValueError(id_ + ': ' + str(exc)) from exc
            estimate = None
        totals[id_] = sum(estimate.values()) if estimate is not None else None
        if not assigned:
            unscheduled.append(id_); continue
        if date.fromisoformat(periods[sprint]['End']) < as_of:
            raise ValueError(id_ + ': unfinished work assigned to a past Sprint; reschedule it')
        for role, amount in estimate.items():
            if (sprint,role) not in caps:
                raise ValueError(f'{id_}: no capacity row for {role}, Sprint {sprint}')
            caps[(sprint,role)]['effort'] += amount
            if amount: caps[(sprint,role)]['items'].append(id_)
        if state == 'blocked': messages.append(t('blocked', id=id_))
        ready_after = meta.get('ready_after')
        if ready_after:
            ready_date = date.fromisoformat(ready_after)
            start = date.fromisoformat(periods[sprint]['Start']); end = date.fromisoformat(periods[sprint]['End'])
            if ready_date > end: messages.append(t('external_late', id=id_))
            elif ready_date > max(start, as_of): messages.append(t('external_wait', id=id_))
        if meta.get('due_date') and date.fromisoformat(periods[sprint]['End']) > date.fromisoformat(meta['due_date']):
            messages.append(t('deadline', id=id_))
    done, visiting = set(), set()
    def visit(key):
        if key in visiting: raise ValueError('Cyclic PBI dependencies')
        if key in done: return
        visiting.add(key)
        for dep in graph[key]: visit(dep)
        visiting.remove(key); done.add(key)
    for id_ in graph: visit(id_)
    for id_, deps in graph.items():
        meta = ids[id_][1]
        if meta['delivery_status'] in {'done','cancelled'} or meta.get('sprint') is None: continue
        for dep in deps:
            dm = ids[dep][1]
            if dm['delivery_status'] == 'done': continue
            if dm['delivery_status'] == 'cancelled': messages.append(t('dep_cancelled', id=id_, dep=dep))
            elif dm.get('sprint') is None: messages.append(t('dep_unscheduled', id=id_, dep=dep))
            elif dm['sprint'] >= meta['sprint']:
                messages.append(t('dep_later' if dm['sprint'] > meta['sprint'] else 'dep_same', id=id_, dep=dep))
    return {'caps':caps,'periods':periods,'messages':messages,'totals':totals,'original':original,
            'unscheduled':unscheduled,'as_of':as_of,'pbis':ids}


def write_plan(root):
    # Resolve the renderer before writing; an unsupported locale never overwrites a prior report.
    t = translator(root)
    language = language_settings(root)['document_language']
    target = root / '06_Delivery/Plan-Report.md'
    meta = {'id':'REPORT-PLAN','type':'report','language':language,'workflow_status':'forecast','generated_at':stamp()}
    try: plan = calculate(root)
    except (ValueError, OSError, TypeError, KeyError) as exc:
        meta['workflow_status'] = 'blocked'
        save_note(target, meta, '# '+t('plan_title')+'\n\n'+t('plan_failed',error=str(exc)))
        raise
    assigned = sum(v for k,v in plan['totals'].items() if k not in plan['unscheduled'] and v is not None)
    unassigned = sum(plan['totals'][k] or 0 for k in plan['unscheduled'])
    unknown = sum(plan['totals'][k] is None for k in plan['unscheduled'])
    body = '# '+t('plan_title')+'\n\n'+t('plan_intro',date=plan['as_of'],assigned=assigned,unassigned=unassigned,unknown=unknown)+'\n\n'
    scheduled_active = any(m.get('sprint') is not None and m.get('delivery_status') not in {'done','cancelled'} for _,m,_ in plan['pbis'].values())
    if not scheduled_active: body += '> '+t('empty_schedule')+'\n\n'
    body += t('plan_basis')+'\n\n| Sprint | Start | End | Goal |\n|---|---|---|---|\n'
    for _, row in sorted(plan['periods'].items()):
        body += '| ' + ' | '.join(row[k] for k in ['Sprint','Start','End','Goal']) + ' |\n'
    body += '\n'+t('capacity_header')+'\n|---|---|---:|---:|---:|---|\n'
    for (s,role), data in sorted(plan['caps'].items()):
        gap = data['capacity']-data['effort']
        body += f'| {s} | {role} | {data["effort"]:.2f} | {data["capacity"]:.2f} | {gap:.2f} | {t("overload" if gap < -1e-9 else "within")} |\n'
    body += '\n## '+t('items_title')+'\n\n'+t('items_header')+'\n|---|---|---:|---:|---|\n'
    for id_, (p,m,b) in plan['pbis'].items():
        initial = plan['original'].get(id_); rem = plan['totals'][id_]
        body += '| '+' | '.join([link(root,p),m['delivery_status'],f'{initial:.2f}' if initial is not None else t('not_counted'),f'{rem:.2f}' if rem is not None else t('unestimated'),str(m.get('sprint') or t('unscheduled'))])+' |\n'
    body += '\n## '+t('dependencies')+'\n\n'+('\n'.join('- '+m for m in plan['messages']) or t('no_conflict'))
    body += '\n\n## '+t('decisions')+'\n\n'+t('plan_decisions')
    save_note(target, meta, body)
    return target
