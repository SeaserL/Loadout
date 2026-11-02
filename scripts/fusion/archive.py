# SPDX-License-Identifier: Apache-2.0
"""Create/inspect the one candidate; extraction is confined to this run."""
from pathlib import Path
import hashlib, json, os, sys, zipfile

repo = Path(__file__).resolve().parents[2]
root = Path(os.environ['FUSION_RELEASE_ROOT']).resolve()
assert root.is_relative_to(repo / 'development/fusion/runs')
def read(p): return json.loads(p.read_text(encoding='utf-8-sig'))
def write(p,v): p.write_text(json.dumps(v,indent=2),encoding='utf-8')
def digest(p):
    with p.open('rb') as f: return hashlib.file_digest(f,'sha256').hexdigest()
def extract(archive, dest):
    dest.mkdir()
    with zipfile.ZipFile(archive) as z:
        for entry in z.infolist():
            assert (dest/entry.filename).resolve().is_relative_to(dest.resolve())
        z.extractall(dest)
mode=sys.argv[1]
if mode in ['baseline','current']:
    receipt=repo/'development/fusion/current.json'
    if not receipt.exists(): receipt=repo/'development/experiments/omniroute-phase17/evidence/release.json'
    old=read(receipt);archive=Path(old['archive'])
    assert digest(archive)==old['archiveSha256'], 'BASELINE_HASH'
    dest=root/('baseline' if mode=='baseline' else 'extracted')
    extract(archive,dest)
    write(root/'runs'/('old-baseline.json' if mode=='baseline' else 'release-latest.json'),
          {**old,'directory':str(dest),'extracted':str(dest)})
elif mode=='archive':
    r=read(root/'runs/package-latest-v52.json');src=Path(r['directory']).resolve()
    standalone=(repo/'development/upstreams/omniroute/.claude/worktrees/loadout-phase11-v52/.build/loadout-fusion/standalone').resolve()
    assert src.is_relative_to(root) or src==standalone
    archive=root/(r['releaseId']+'.zip')
    with zipfile.ZipFile(archive,'x',compression=zipfile.ZIP_DEFLATED,compresslevel=1,allowZip64=True) as z:
        for f in sorted(src.rglob('*')):
            assert not f.is_symlink()
            if f.is_file(): z.write(f,f.relative_to(src).as_posix())
    # Verify every archived byte without retaining another unpacked package.
    manifest=read(src/'fusion-manifest.json')
    expected={f['path']:f['sha256'] for f in manifest['files']}
    expected['fusion-manifest.json']=digest(src/'fusion-manifest.json')
    with zipfile.ZipFile(archive) as z:
        assert set(z.namelist())==set(expected), 'ARCHIVE_MEMBERS'
        assert len(z.namelist())==len(expected), 'ARCHIVE_DUPLICATES'
        for name,sha in expected.items():
            with z.open(name) as f: assert hashlib.file_digest(f,'sha256').hexdigest()==sha, 'ARCHIVE_CONTENT'
    dest=src
    write(root/'runs/release-latest.json',{**r,'archive':str(archive),'archiveSha256':digest(archive),
        'bytes':archive.stat().st_size,'extracted':str(dest)})
else: raise ValueError('UNKNOWN_ACTION')
print(mode+' complete')
