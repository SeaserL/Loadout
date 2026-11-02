# SPDX-License-Identifier: Apache-2.0
"""Private input for the 059 migration. Reuses the existing Windows reader."""
import importlib.util
import json
import os
import sqlite3
import sys
from pathlib import Path

repo = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location('legacy_source', repo/'development/experiments/omniroute-phase4/source.py')
legacy = importlib.util.module_from_spec(spec)
spec.loader.exec_module(legacy)
ALLOWED = {'sensenova', 'commandcode', 'deepseek', 'opencode-go'}

def collect(request):
    db = sqlite3.connect(Path(request['source']).resolve().as_uri()+'?mode=ro', uri=True)
    db.row_factory = sqlite3.Row
    db.execute('PRAGMA query_only=ON')
    db.execute('BEGIN')
    counts, rows = {}, []
    for a in db.execute('''SELECT a.id,a.alias,a.enabled,a.position,a.transport_opt_in_at,p.slug,p.kind,p.base_url
        FROM accounts a JOIN providers p ON p.id=a.provider_id ORDER BY p.slug,a.position,a.id'''):
        counts[a['slug']] = counts.get(a['slug'], 0)+1
        ref = a['slug']+'/account-'+str(counts[a['slug']])
        credentials = list(db.execute('SELECT credential_type,source,secret_ref FROM credentials WHERE account_id=?', (a['id'],)))
        keys = [c for c in credentials if c['credential_type']=='api_key' and c['source']=='user' and c['secret_ref']]
        row = {'ref':ref,'alias':a['alias'],'provider':a['slug'],'kind':a['kind'],'enabled':bool(a['enabled']),
               'baseUrl':a['base_url'],'apiKeyAvailable':len(keys)==1,'credentialKinds':[c['credential_type'] for c in credentials],
               'transportOptIn':a['transport_opt_in_at'] is not None}
        if request['operation']=='private-copy' and ref in request['refs']:
            if not a['enabled'] or a['kind'] not in ALLOWED or len(keys)!=1:
                raise RuntimeError('SOURCE_SELECTION_UNSUPPORTED')
            row['key'] = legacy.credential(keys[0]['secret_ref'])
        rows.append(row)
    db.close()
    if request['operation']=='private-copy':
        rows=[r for r in rows if r['ref'] in request['refs']]
        if len(rows)!=len(set(request['refs'])): raise RuntimeError('SOURCE_SELECTION_MISMATCH')
    return rows

if __name__=='__main__':
    try:
        if sys.stdout.isatty() or os.environ.get('LOADOUT_LIVE_TESTS')!='1': raise RuntimeError('PRIVATE_PIPE_REQUIRED')
        request=json.loads(sys.stdin.read())
        if request['operation'] not in ['metadata','private-copy']: raise RuntimeError('UNKNOWN_OPERATION')
        sys.stdout.write(json.dumps(collect(request)))
    except Exception:
        sys.stderr.write('SOURCE_BINDING_INPUT_FAILED')
        sys.exit(1)
