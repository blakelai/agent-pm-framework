from datetime import date

from .vault import catalog, link, read_note, save_note, stamp


def status_report(root):
    config, _ = read_note(root/'06_Delivery/Planning.md')
    as_of = date.fromisoformat(config['as_of']) if config.get('as_of') else None
    records = catalog(root)
    counts = {}
    for _, m, _ in records:
        if m.get('type') == 'pbi':
            state = m.get('delivery_status', 'unknown')
            counts[state] = counts.get(state, 0)+1
    label = str(as_of) if as_of else 'not configured; set Planning.as_of before date-based assessment'
    body = f'# Project status\n\nAs of: {label}. This report summarizes recorded states; completed item counts do not measure business-value percentage.\n\n'
    body += (', '.join(f'{k}: {v}' for k,v in sorted(counts.items())) or 'No backlog items recorded.') + '\n\n'
    body += '## Open follow-up\n\n| Item | Type | Owner | Due date | Signal |\n|---|---|---|---|---|\n'
    for path, m, _ in records:
        if m.get('type') not in {'risk','issue','action','question','decision'} or m.get('workflow_status') in {'closed','resolved','accepted','rejected','rescinded'}:
            continue
        due = m.get('due_date')
        hint = 'No due date' if not due else 'Forecast date not configured' if as_of is None else 'Overdue' if date.fromisoformat(due)<as_of else 'Open'
        body += '| ' + ' | '.join([link(root,path),m['type'],str(m.get('owner','Unassigned')),str(due or 'TBD'),hint]) + ' |\n'
    body += '\n## Management judgment\n\nThe agent adds recommendations after reviewing changes, risks, dependencies and forecasts. This tool neither prioritizes work nor sends reports.\n\n'
    for relative in ['06_Delivery/Plan-Report.md','90_Agent/Reports/Traceability-Report.md']:
        if (root/relative).is_file(): body += '- '+link(root,root/relative)+'\n'
    target=root/'90_Agent/Reports/Status-Report.md'
    save_note(target,{'id':'REPORT-STATUS','type':'report','language':'en','generated_at':stamp()},body)
    return target
