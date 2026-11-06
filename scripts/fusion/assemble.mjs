// SPDX-License-Identifier: Apache-2.0
import fs from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";
import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import {
  root,
  repo,
  source,
  tools,
  dist,
  profile,
  upstream,
} from "./config.mjs";
const version = "52";
const build = JSON.parse(
  fs.readFileSync(path.join(root, "runs/build-latest-v" + version + ".json")),
);
if (JSON.parse(fs.readFileSync(path.join(build.run, "result.json"))).code !== 0)
  throw Error("BUILD_FAILED");
const sha = (x) => createHash("sha256").update(x).digest("hex");
const releaseId = "fusion-v3.8." + version + "-isolated-" + profile;
// What gets packaged must be what the repository records.
const recorded = spawnSync("python", ["scripts/fusion/backend-source.py", "check"], {
  cwd: repo,
  encoding: "utf8",
  windowsHide: true,
});
if (recorded.status !== 0) throw Error("BACKEND_SOURCE_NOT_EXPORTED\n" + recorded.stdout);
const productVersion = JSON.parse(
  fs.readFileSync(path.join(repo, "apps/fusion-desktop/tauri.conf.json"), "utf8"),
).version;
const inPlace = process.env.FUSION_ASSEMBLE_IN_PLACE === "1";
const refreshFrom = process.env.FUSION_REFRESH_FROM;
if (
  !refreshFrom &&
  fs.existsSync(path.join(source, dist, "standalone/fusion-manifest.json"))
)
  throw Error("SOURCE_ALREADY_ASSEMBLED_REBUILD_REQUIRED");
if (refreshFrom && !inPlace) throw Error("REFRESH_REQUIRES_IN_PLACE");
const directory = inPlace
  ? path.resolve(process.env.FUSION_ASSEMBLE_DIRECTORY ?? path.join(source, dist, "standalone"))
  : path.join(root, "packages", releaseId + "-" + Date.now());
if (process.env.FUSION_ASSEMBLE_DIRECTORY && (!refreshFrom || !directory.startsWith(root + path.sep)))
  throw Error("ASSEMBLY_DIRECTORY_SCOPE");
if (!inPlace)
  fs.cpSync(path.join(source, dist, "standalone"), directory, {
    recursive: true,
    dereference: true,
  });
if (refreshFrom) {
  // Reuse compiled backend only when both the sealed files and its source evidence match.
  // Verify in a child so Windows releases the native keytar DLL before copying.
  const verified = spawnSync(
    process.execPath,
    [
      "--input-type=module",
      "-e",
      `const {verifyPackage}=await import(${JSON.stringify(pathToFileURL(path.join(directory, "tools/runtime.mjs")).href)});verifyPackage();`,
    ],
    { encoding: "utf8", windowsHide: true },
  );
  if (verified.status !== 0) throw Error("REFRESH_PACKAGE_INVALID");
  const patch = spawnSync("git", ["-C", source, "diff", "--binary"], {
    encoding: "utf8",
    windowsHide: true,
    maxBuffer: 4 * 1024 * 1024,
  });
  if (
    patch.status !== 0 ||
    patch.stdout !==
      fs.readFileSync(
        path.join(refreshFrom, "evidence/v52/upstream.patch"),
        "utf8",
      )
  )
    throw Error("COMPILED_SOURCE_CHANGED");
  function compare(dir, original) {
    const entries = fs.readdirSync(dir, { withFileTypes: true });
    if (entries.length !== fs.readdirSync(original).length)
      throw Error("COMPILED_ADDITIONS_CHANGED");
    for (const e of entries) {
      const a = path.join(dir, e.name),
        b = path.join(original, e.name);
      if (e.isDirectory()) compare(a, b);
      else if (
        !e.isFile() ||
        sha(fs.readFileSync(a)) !== sha(fs.readFileSync(b))
      )
        throw Error("COMPILED_ADDITIONS_CHANGED");
    }
  }
  for (const name of ["src/lib/loadoutPhase1", "src/app/api/loadout"])
    compare(
      path.join(source, name),
      path.join(refreshFrom, "evidence/v52/source-additions", name),
    );
  const old = JSON.parse(
    fs.readFileSync(path.join(directory, "fusion-manifest.json")),
  );
  for (const f of old.files.filter(
    (f) => f.path.startsWith("public/loadout/") || f.path.startsWith("tools/"),
  )) {
    const target = path.resolve(directory, f.path);
    if (!target.startsWith(path.resolve(directory) + path.sep))
      throw Error("REFRESH_SCOPE");
    fs.unlinkSync(target);
  }
  fs.unlinkSync(path.join(directory, "fusion-manifest.json"));
}
for (const name of ["keytar", "node-addon-api"])
  fs.cpSync(
    path.join(tools, name),
    path.join(directory, "node_modules", name),
    { recursive: true, dereference: true },
  );
fs.copyFileSync(process.execPath, path.join(directory, "node.exe"));
const serverEntry = path.join(directory, "server.js");
if (
  !fs
    .readFileSync(serverEntry, "utf8")
    .includes(
      "// Distribution entry guard: reject accidental native startup before loading Next.",
    )
)
  fs.writeFileSync(
    serverEntry,
    `// SPDX-License-Identifier: Apache-2.0
// Distribution entry guard: reject accidental native startup before loading Next.
if (process.env.LOADOUT_FUSION_LAUNCHER !== "1" || process.env.HOSTNAME !== "127.0.0.1" || !/[\\\\/]loadout-fusion-isolated[\\\\/]/i.test(process.env.DATA_DIR || "") || !/^loadout-test:omniroute-phase3-[a-f0-9-]+$/.test(process.env.LOADOUT_TEST_SERVICE || "")) {
  process.stderr.write("USE_FUSION_LAUNCHER\\n"); process.exit(64);
}
` + fs.readFileSync(serverEntry, "utf8"),
  );
fs.cpSync(path.join(repo, "fusion"), path.join(directory, "tools"), {
  recursive: true,
});
// Same-origin static console; no second HTTP service or API adapter.
fs.cpSync(
  path.join(repo, ".build/fusion-console"),
  path.join(directory, "public/loadout"),
  {
    recursive: true,
  },
);
const consoleNotices = path.join(directory, "public/loadout/licenses");
fs.mkdirSync(consoleNotices, { recursive: true });
for (const name of ["react", "react-dom", "lucide-react"])
  fs.copyFileSync(
    path.join(repo, "node_modules", name, "LICENSE"),
    path.join(consoleNotices, name + ".txt"),
  );
fs.copyFileSync(
  path.join(source, "LICENSE"),
  path.join(consoleNotices, "OmniRoute.txt"),
);
fs.copyFileSync(
  path.join(repo, "fusion/fusion.cmd"),
  path.join(directory, "fusion.cmd"),
);
for (const name of ["Start-Loadout.cmd", "Stop-Backend.cmd"])
  fs.copyFileSync(path.join(repo, "fusion", name), path.join(directory, name));
fs.copyFileSync(path.join(repo, ".build/fusion-desktop-target/release/Loadout.exe"), path.join(directory, "Loadout.exe"));
fs.copyFileSync(
  path.join(repo, "fusion/README.md"),
  path.join(directory, "README-FUSION.md"),
);
for (const name of ["LICENSE", "THIRD_PARTY_NOTICES.md"])
  fs.copyFileSync(path.join(source, name), path.join(directory, name));
fs.copyFileSync(
  path.join(repo, "LICENSE"),
  path.join(directory, "Loadout-ADAPTER-LICENSE.txt"),
);
fs.copyFileSync(
  path.join(repo, "THIRD_PARTY_NOTICES.md"),
  path.join(directory, "Loadout-THIRD_PARTY_NOTICES.md"),
);
fs.copyFileSync(
  path.join(repo, "development/fusion/notices/Node-v24.22.1-LICENSE.txt"),
  path.join(directory, "Node-LICENSE.txt"),
);
// ExecPlan 071: the AI pack goes into the same folder, then what no edition uses comes out.
const { stage } = await import("./ai-pack.mjs");
await stage(directory);
const { unused, inAiPack, PACK_FILE } = await import("./editions.mjs");
const packVersion = JSON.parse(
  fs.readFileSync(path.join(repo, "development/fusion/ai-models.json"), "utf8"),
).packVersion;
function prune(dir) {
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const file = path.join(dir, entry.name);
    const rel = path.relative(directory, file).replaceAll("\\", "/");
    if (entry.isDirectory()) {
      prune(file);
      if (fs.readdirSync(file).length === 0) fs.rmdirSync(file);
    } else if (unused(rel)) fs.unlinkSync(file);
  }
}
prune(directory);
const files = [];
function walk(dir) {
  for (const entry of fs
    .readdirSync(dir, { withFileTypes: true })
    .sort((a, b) => a.name.localeCompare(b.name))) {
    const file = path.join(dir, entry.name);
    if (entry.isDirectory()) walk(file);
    else if (entry.isFile() && dir === directory && entry.name === PACK_FILE) continue;
    else if (entry.isFile())
      files.push({
        path: path.relative(directory, file).replaceAll("\\", "/"),
        sha256: sha(fs.readFileSync(file)),
      });
    else throw Error("PACKAGE_LINK");
  }
}
walk(directory);
// The app is every file outside the AI pack: the lite edition. The pack lists itself.
const packFiles = files.filter((f) => inAiPack(f.path));
files.splice(0, files.length, ...files.filter((f) => !inAiPack(f.path)));
const manifest = {
  releaseId,
  productVersion,
  upstream:
    version === "52"
      ? "ad6f33f66bda34eba98645d3bfdabbde294bc843"
      : "c1e30b7676975feb298b49eff6ff58923c04b89e",
  upstreamVersion: "3.8." + version,
  adapterProfile: profile,
  workbuddy: "25680ca7f7ff5d7b84bb35f45de766519fccbc9d",
  node: process.version,
  aiPack: packVersion,
  treeHash: sha(JSON.stringify(files)),
  files,
};
manifest.releaseId += "-" + manifest.treeHash.slice(0, 12);
fs.writeFileSync(
  path.join(directory, "fusion-manifest.json"),
  JSON.stringify(manifest, null, 2),
);
fs.writeFileSync(
  path.join(directory, PACK_FILE),
  JSON.stringify(
    { packVersion, productVersion, treeHash: sha(JSON.stringify(packFiles)), files: packFiles },
    null,
    2,
  ),
);
fs.mkdirSync(path.join(root, "evidence/v" + version), { recursive: true });
const diff = spawnSync("git", ["-C", source, "diff", "--binary"], {
  encoding: "utf8",
  windowsHide: true,
  maxBuffer: 4 * 1024 * 1024,
});
if (diff.status !== 0) throw Error("PATCH_EXPORT_FAILED");
fs.writeFileSync(
  path.join(root, "evidence/v" + version + "/upstream.patch"),
  diff.stdout,
);
for (const name of ["src/lib/loadoutPhase1", "src/app/api/loadout"])
  fs.cpSync(
    path.join(source, name),
    path.join(root, "evidence/v" + version + "/source-additions", name),
    { recursive: true },
  );
const receipt = {
  directory,
  releaseId: manifest.releaseId,
  files: files.length,
  packFiles: packFiles.length,
  packVersion,
  treeHash: manifest.treeHash,
};
fs.writeFileSync(
  path.join(root, "runs/package-latest-v" + version + ".json"),
  JSON.stringify(receipt, null, 2),
);
console.log(JSON.stringify(receipt));
