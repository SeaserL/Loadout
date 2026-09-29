# SPDX-License-Identifier: Apache-2.0
"""Third-party license bundle and policy check (ExecPlan 007 §2).

Collects every crate linked into the shipped binaries (normal dependencies of the
`loadout` and `loadout-desktop` packages on x86_64-pc-windows-msvc) and the web UI's
runtime npm packages, checks their licenses against the allow list, and writes
THIRD_PARTY_LICENSES.md with the full license texts.

Usage:
  python scripts/third_party_licenses.py --check              # policy check only
  python scripts/third_party_licenses.py --out <file>         # check and write bundle
"""

import argparse
import hashlib
import json
import os
import re
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
TARGET = "x86_64-pc-windows-msvc"
ROOT_PACKAGES = ("loadout", "loadout-desktop")

# Reference map §5 / §14: permissive licenses; MPL-2.0 for unmodified dependencies.
ALLOWED = {
    "MIT",
    "MIT-0",
    "Apache-2.0",
    "BSD-2-Clause",
    "BSD-3-Clause",
    "ISC",
    "Zlib",
    "Unicode-3.0",
    "Unicode-DFS-2016",
    "CDLA-Permissive-2.0",
    "BSL-1.0",
    "CC0-1.0",
    "0BSD",
    "Unlicense",
    "MPL-2.0",
}

LICENSE_FILE = re.compile(r"^(LICEN[CS]E|COPYING|NOTICE|COPYRIGHT|UNLICENSE)", re.IGNORECASE)


def spdx_allowed(expr):
    """Evaluates an SPDX expression against ALLOWED (OR: any side; AND: both sides)."""
    if not expr:
        return False
    expr = expr.replace("/", " OR ")
    tokens = re.findall(r"\(|\)|[A-Za-z0-9.+\-]+", expr)
    pos = 0

    def primary():
        nonlocal pos
        tok = tokens[pos]
        pos += 1
        if tok == "(":
            value = disjunction()
            pos += 1  # ")"
            return value
        ok = tok.rstrip("+") in ALLOWED
        if pos < len(tokens) and tokens[pos] == "WITH":
            pos += 2  # exception applies to the preceding license
        return ok

    def conjunction():
        nonlocal pos
        value = primary()
        while pos < len(tokens) and tokens[pos] == "AND":
            pos += 1
            value = primary() and value
        return value

    def disjunction():
        nonlocal pos
        value = conjunction()
        while pos < len(tokens) and tokens[pos] == "OR":
            pos += 1
            value = conjunction() or value
        return value

    try:
        return disjunction()
    except IndexError:
        return False


def license_texts(directory):
    texts = []
    try:
        names = sorted(os.listdir(directory))
    except OSError:
        return texts
    for name in names:
        path = os.path.join(directory, name)
        if os.path.isfile(path) and LICENSE_FILE.match(name):
            with open(path, encoding="utf-8", errors="replace") as f:
                texts.append((name, f.read().strip()))
    return texts


def cargo_packages(cargo):
    meta = json.loads(
        subprocess.check_output(
            [cargo, "metadata", "--format-version", "1", "--filter-platform", TARGET],
            cwd=ROOT,
        )
    )
    packages = {p["id"]: p for p in meta["packages"]}
    members = set(meta["workspace_members"])
    nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}
    roots = [pid for pid in members if packages[pid]["name"] in ROOT_PACKAGES]
    seen, stack = set(), list(roots)
    while stack:
        pid = stack.pop()
        if pid in seen:
            continue
        seen.add(pid)
        for dep in nodes[pid]["deps"]:
            # Normal dependencies only (kind null); build/dev code is not shipped.
            if any(k["kind"] is None for k in dep["dep_kinds"]):
                stack.append(dep["pkg"])
    out = []
    for pid in seen - members:
        p = packages[pid]
        directory = os.path.dirname(p["manifest_path"])
        texts = license_texts(directory)
        if p.get("license_file"):
            lf = os.path.join(directory, p["license_file"])
            if os.path.isfile(lf) and not any(n == os.path.basename(lf) for n, _ in texts):
                with open(lf, encoding="utf-8", errors="replace") as f:
                    texts.append((os.path.basename(lf), f.read().strip()))
        out.append(
            {
                "ecosystem": "crates.io",
                "name": p["name"],
                "version": p["version"],
                "license": p.get("license") or "",
                "source": p.get("repository") or f"https://crates.io/crates/{p['name']}",
                "texts": texts,
            }
        )
    return out


def npm_packages():
    lock_path = os.path.join(ROOT, "package-lock.json")
    with open(lock_path, encoding="utf-8") as f:
        lock = json.load(f)
    out = []
    for key, info in lock.get("packages", {}).items():
        if not key.startswith("node_modules/") or info.get("dev") or info.get("link"):
            continue
        name = key[len("node_modules/"):]
        directory = os.path.join(ROOT, key.replace("/", os.sep))
        out.append(
            {
                "ecosystem": "npm",
                "name": name,
                "version": info.get("version", ""),
                "license": info.get("license", ""),
                "source": f"https://www.npmjs.com/package/{name}",
                "texts": license_texts(directory),
            }
        )
    return out


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--out", help="write the license bundle here")
    parser.add_argument("--check", action="store_true", help="policy check only")
    parser.add_argument("--cargo", default="cargo")
    args = parser.parse_args()

    packages = sorted(
        cargo_packages(args.cargo) + npm_packages(),
        key=lambda p: (p["ecosystem"], p["name"].lower(), p["version"]),
    )
    violations = [p for p in packages if not spdx_allowed(p["license"])]
    missing = [p for p in packages if not p["texts"]]
    for p in violations:
        print(f"NOT ALLOWED: {p['ecosystem']} {p['name']} {p['version']}: {p['license'] or '(none)'}")
    print(f"{len(packages)} third-party packages; {len(violations)} license violations; "
          f"{len(missing)} without a bundled license file")
    if violations:
        return 1
    if args.check or not args.out:
        return 0

    groups = {}
    for p in packages:
        body = "\n\n".join(f"--- {name} ---\n\n{text}" for name, text in p["texts"])
        if not body:
            body = (f"The published package contains no license file. License: {p['license']} "
                    f"(the standard text of each license appears elsewhere in this document). "
                    f"Copyright notices: see {p['source']}.")
        digest = hashlib.sha256(body.encode()).hexdigest()
        groups.setdefault(digest, (body, []))[1].append(p)

    lines = [
        "# Third-Party Licenses",
        "",
        "Loadout (Apache-2.0) includes the following third-party software. Generated by",
        "`scripts/third_party_licenses.py`; code reuse and derived work are recorded separately",
        "in THIRD_PARTY_NOTICES.md.",
        "",
        "| Package | Version | License | Source |",
        "|---|---|---|---|",
    ]
    for p in packages:
        lines.append(f"| {p['name']} ({p['ecosystem']}) | {p['version']} | {p['license']} | {p['source']} |")
    lines += ["", "## License texts", ""]
    for body, members in groups.values():
        names = ", ".join(f"{m['name']} {m['version']}" for m in members)
        lines += [f"### {names}", "", "```text", body.replace("```", "'''"), "```", ""]
    with open(args.out, "w", encoding="utf-8", newline="\n") as f:
        f.write("\n".join(lines))
    print(f"wrote {args.out}")
    return 0


def self_test():
    assert spdx_allowed("MIT OR Apache-2.0")
    assert spdx_allowed("Apache-2.0 WITH LLVM-exception")
    assert spdx_allowed("(MIT OR Apache-2.0) AND Unicode-3.0")
    assert spdx_allowed("MIT/Apache-2.0")
    assert not spdx_allowed("GPL-3.0-only")
    assert not spdx_allowed("MIT AND GPL-2.0")
    assert spdx_allowed("GPL-2.0 OR MIT")
    assert not spdx_allowed("")


if __name__ == "__main__":
    self_test()
    sys.exit(main())
