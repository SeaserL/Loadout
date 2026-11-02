# SPDX-License-Identifier: Apache-2.0
"""The backend's record in this repository: a patch on a pinned OmniRoute plus Loadout's own files.

export   copy the working copy's changes into backend/
prepare  create the working copy from the pin and backend/, cloning the upstream if needed
check    exit 1 if the working copy and backend/ differ
"""
import argparse
import hashlib
import json
import shutil
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
BACKEND = REPO / "backend"
PATCH = BACKEND / "omniroute-v3.8.52.patch"
OWN = ("src/lib/loadoutPhase1", "src/app/api/loadout")
CHECKOUT = REPO / "development/upstreams/omniroute"
SOURCE = CHECKOUT / ".claude/worktrees/loadout-phase11-v52"


def pin():
    lock = json.loads((REPO / "development/upstreams.lock.json").read_text(encoding="utf-8"))
    return next(r for r in lock["repositories"] if r["name"] == "omniroute")


def git(cwd, *args):
    done = subprocess.run(["git", "-C", str(cwd), *args], capture_output=True)
    if done.returncode != 0:
        sys.exit(f"git {' '.join(args)}: {done.stderr.decode('utf-8', 'replace').strip()}")
    return done.stdout


def outside_own(source):
    """Untracked files the record would not carry."""
    entries = git(source, "status", "--porcelain=v1", "-z", "--untracked-files=all").split(b"\0")
    paths = [e[3:].decode("utf-8") for e in entries if e.startswith(b"?? ")]
    return [p for p in paths if not p.startswith(tuple(f"{own}/" for own in OWN))]


def tree(root):
    return {
        p.relative_to(root).as_posix(): hashlib.sha256(p.read_bytes()).hexdigest()
        for p in sorted(root.rglob("*"))
        if p.is_file()
    }


def differences(source):
    found = [f"untracked: {p}" for p in outside_own(source)]
    if not PATCH.exists() or git(source, "diff", "--binary", "HEAD") != PATCH.read_bytes():
        found.append(f"patch differs: {PATCH.name}")
    for own in OWN:
        ours, theirs = tree(BACKEND / own), tree(source / own)
        found += [f"{own}/{name}" for name in sorted(set(ours) | set(theirs)) if ours.get(name) != theirs.get(name)]
    return found


def export():
    extra = outside_own(SOURCE)
    if extra:
        sys.exit("Untracked files outside the recorded folders:\n  " + "\n  ".join(extra))
    BACKEND.mkdir(exist_ok=True)
    PATCH.write_bytes(git(SOURCE, "diff", "--binary", "HEAD"))
    for own in OWN:
        shutil.rmtree(BACKEND / own, ignore_errors=True)
        shutil.copytree(SOURCE / own, BACKEND / own)
    print(f"Recorded {SOURCE.name} in {BACKEND.relative_to(REPO)}")


def prepare(target):
    upstream = pin()
    if target.exists():
        sys.exit(f"Already exists: {target}")
    if not (CHECKOUT / ".git").exists():
        CHECKOUT.parent.mkdir(parents=True, exist_ok=True)
        # Line endings stay as the upstream has them, or the patch would not apply on Windows.
        subprocess.run(
            ["git", "clone", "--filter=blob:none", "--no-checkout", "-c", "core.autocrlf=false", upstream["origin"], str(CHECKOUT)],
            check=True,
        )
    target.parent.mkdir(parents=True, exist_ok=True)
    git(CHECKOUT, "-c", "core.autocrlf=false", "worktree", "add", "--detach", str(target), upstream["revision"])
    git(target, "-c", "core.autocrlf=false", "apply", "--whitespace=nowarn", str(PATCH))
    for own in OWN:
        shutil.copytree(BACKEND / own, target / own)
    print(f"Prepared {target} at {upstream['tag']} ({upstream['revision'][:12]})")


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("action", choices=["export", "prepare", "check"])
    parser.add_argument("--path", type=Path, help="working copy to prepare or check (default: the build's)")
    args = parser.parse_args()
    source = args.path.resolve() if args.path else SOURCE
    if args.action == "export":
        export()
    elif args.action == "prepare":
        prepare(source)
    else:
        found = differences(source)
        if found:
            print("backend/ differs from the working copy; run: python scripts/fusion/backend-source.py export")
            print("\n".join(f"  {f}" for f in found))
            sys.exit(1)
        print("backend/ matches the working copy")


if __name__ == "__main__":
    main()
