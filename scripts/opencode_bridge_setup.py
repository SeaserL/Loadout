# SPDX-License-Identifier: Apache-2.0
"""Deployment secret handling; read is only for a private child pipe."""
import json
import os
from pathlib import Path
import secrets
import subprocess
import sys
from workbuddy_bridge_key import protect

root = Path(os.environ['LOADOUT_OPENCODE_DIR'])
path = root / 'secrets.dpapi'
if sys.argv[1] == 'init':
    if not path.exists():
        with path.open('xb') as out:
            out.write(protect(json.dumps({'official': secrets.token_urlsafe(32), 'bridge': secrets.token_urlsafe(32)}).encode()))
    print('Protected keys ready')
elif sys.argv[1] == 'read':
    sys.stdout.write(protect(path.read_bytes(), True).decode())
elif sys.argv[1] == 'account':
    key = json.loads(protect(path.read_bytes(), True))['bridge']

    def loadout(*args, key_input=False):
        return subprocess.run([sys.argv[2], '--json', *args], input=key + '\n' if key_input else '',
            capture_output=True, text=True, encoding='utf-8', timeout=60)

    def listed(*args, field):
        result = loadout(*args)
        if result.returncode:
            print('Loadout query failed; is the daemon running?')
            sys.exit(result.returncode)
        return {item[field] for item in json.loads(result.stdout)}

    # A fresh machine may already have the provider or the account with an unrelated key.
    if 'opencode-free' not in listed('provider', 'list', field='slug'):
        result = loadout('provider', 'add', 'opencode-free', '--kind', 'opencode-bridge')
        print('Provider setup exit:', result.returncode)
        if result.returncode:
            sys.exit(result.returncode)
    if 'main' in listed('account', 'list', 'opencode-free', field='alias'):
        result = loadout('account', 'set-key', 'opencode-free', 'main', '--api-key-stdin', key_input=True)
        if not result.returncode:
            result = loadout('model', 'sync', 'opencode-free', 'main')
    else:
        result = loadout('account', 'add', 'opencode-free', 'main', '--api-key-stdin', key_input=True)
    print('Account setup exit:', result.returncode)
    if result.returncode:
        print('Account setup failed; secret and raw response suppressed')
    sys.exit(result.returncode)
else:
    raise SystemExit('Unknown action')
