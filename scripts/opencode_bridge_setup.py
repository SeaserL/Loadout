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
    result = subprocess.run([sys.argv[2], '--json', 'account', 'add', 'opencode-free', 'main', '--api-key-stdin'],
        input=key + '\n', capture_output=True, text=True, encoding='utf-8', timeout=60)
    print('Account setup exit:', result.returncode)
    if result.returncode:
        print('Account setup failed; secret and raw response suppressed')
    sys.exit(result.returncode)
else:
    raise SystemExit('Unknown action')
