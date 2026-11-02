# SPDX-License-Identifier: Apache-2.0
"""Reuse the already verified candidate's Node runtime instead of a moving host runtime."""
from pathlib import Path
import hashlib, json, zipfile
repo=Path(__file__).resolve().parents[2]
receipt=repo/'development/fusion/current.json'
if not receipt.exists(): receipt=repo/'development/experiments/omniroute-phase17/evidence/release.json'
r=json.loads(receipt.read_text(encoding='utf-8-sig'))
def sha(p):
    with p.open('rb') as f: return hashlib.file_digest(f,'sha256').hexdigest()
archive=Path(r['archive']);assert sha(archive)==r['archiveSha256'],'RUNTIME_ARCHIVE_HASH'
target=repo/'development/fusion/tools/node.exe'
with zipfile.ZipFile(archive) as z:
    m=json.loads(z.read('fusion-manifest.json'));assert m['node']=='v24.22.1','RUNTIME_VERSION'
    expected=next(f['sha256'] for f in m['files'] if f['path']=='node.exe')
    if not target.exists(): target.write_bytes(z.read('node.exe'))
    assert sha(target)==expected,'RUNTIME_HASH'
print('Pinned Node runtime verified')
