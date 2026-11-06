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
    # One folder, three archives (ExecPlan 071): the full edition is everything; lite is the app the
    # manifest lists; the AI pack is what ai-pack.json lists. Level 9: 14% smaller than level 1.
    manifest=read(src/'fusion-manifest.json');pack=read(src/'ai-pack.json')
    app={f['path']:f['sha256'] for f in manifest['files']};app['fusion-manifest.json']=digest(src/'fusion-manifest.json')
    packed={f['path']:f['sha256'] for f in pack['files']};packed['ai-pack.json']=digest(src/'ai-pack.json')
    assert not set(app)&set(packed), 'PACK_OVERLAP'
    on_disk={f.relative_to(src).as_posix() for f in src.rglob('*') if f.is_file()}
    assert on_disk==set(app)|set(packed), 'FOLDER_NOT_APP_PLUS_PACK'
    def make(name, members):
        archive=root/name
        with zipfile.ZipFile(archive,'x',compression=zipfile.ZIP_DEFLATED,compresslevel=9,allowZip64=False) as z:
            for rel in sorted(members):
                f=src/rel
                assert not f.is_symlink()
                z.write(f,rel)
        # Verify every archived byte without retaining another unpacked package.
        with zipfile.ZipFile(archive) as z:
            assert set(z.namelist())==set(members), 'ARCHIVE_MEMBERS'
            assert len(z.namelist())==len(members), 'ARCHIVE_DUPLICATES'
            for n,sha in members.items():
                with z.open(n) as f: assert hashlib.file_digest(f,'sha256').hexdigest()==sha, 'ARCHIVE_CONTENT'
        return archive
    full=make(r['releaseId']+'.zip',{**app,**packed})
    lite=make(r['releaseId']+'-lite.zip',app)
    ai=make(r['releaseId']+'-ai-'+pack['packVersion']+'.zip',packed)
    dest=src
    write(root/'runs/release-latest.json',{**r,'archive':str(full),'archiveSha256':digest(full),
        'bytes':full.stat().st_size,'extracted':str(dest),
        'liteArchive':str(lite),'liteSha256':digest(lite),'liteBytes':lite.stat().st_size,
        'packArchive':str(ai),'packSha256':digest(ai),'packBytes':ai.stat().st_size,'packVersion':pack['packVersion']})
else: raise ValueError('UNKNOWN_ACTION')
print(mode+' complete')
