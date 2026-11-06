// SPDX-License-Identifier: Apache-2.0
// The full edition's AI pack (ExecPlan 071): pinned models and the local text-recognition packages.
//   node scripts/fusion/ai-pack.mjs fetch [--record]   download into development/fusion/ai-cache
//   node scripts/fusion/ai-pack.mjs stage <package>    copy into a package folder being assembled
// Models are read from the pinned revision and must match development/fusion/ai-models.json;
// `--record` fills in hashes that are still null (commit the result).
import fs from "node:fs";
import path from "node:path";
import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import { Readable } from "node:stream";
import { pipeline } from "node:stream/promises";
import { fileURLToPath } from "node:url";

const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const pinsFile = path.join(repo, "development/fusion/ai-models.json");
const cache = path.join(repo, "development/fusion/ai-cache");
const npmProject = path.join(repo, "development/fusion/ai-pack");
/** Packages the OCR needs beyond what the backend already ships. */
export const OCR_PACKAGES = [
  "@gutenye/ocr-node",
  "@gutenye/ocr-common",
  "@gutenye/ocr-models",
  "@techstark/opencv-js",
  "js-clipper",
  "tiny-invariant",
];

const pins = () => JSON.parse(fs.readFileSync(pinsFile, "utf8"));
const cached = (model, file) => path.join(cache, model.repo, file);

async function sha256(file) {
  const hash = createHash("sha256");
  await pipeline(fs.createReadStream(file), hash);
  return hash.digest("hex");
}

async function download(url, target) {
  const response = await fetch(url, { headers: { "user-agent": "Loadout-build" }, signal: AbortSignal.timeout(30 * 60 * 1000) });
  if (!response.ok || !response.body) throw Error(`HTTP ${response.status} for ${url}`);
  fs.mkdirSync(path.dirname(target), { recursive: true });
  const part = target + ".part";
  await pipeline(Readable.fromWeb(response.body), fs.createWriteStream(part));
  fs.renameSync(part, target);
}

export async function fetchModels({ record = false } = {}) {
  const data = pins();
  let changed = false;
  for (const model of data.models) {
    for (const [file, expected] of Object.entries(model.files)) {
      const target = cached(model, file);
      if (fs.existsSync(target) && expected && (await sha256(target)) === expected) continue;
      const url = `https://huggingface.co/${model.repo}/resolve/${model.revision}/${file}`;
      console.log(`fetch ${model.repo}/${file}`);
      await download(url, target);
      const actual = await sha256(target);
      if (expected === null && record) {
        model.files[file] = actual;
        changed = true;
      } else if (actual !== expected) {
        fs.rmSync(target, { force: true });
        throw Error(`HASH_MISMATCH ${model.repo}/${file}: ${actual}`);
      }
    }
  }
  if (changed) fs.writeFileSync(pinsFile, JSON.stringify(data, null, 2) + "\n");
  const unrecorded = data.models.flatMap((m) => Object.entries(m.files).filter(([, h]) => !h).map(([f]) => `${m.repo}/${f}`));
  if (unrecorded.length) throw Error("UNRECORDED_HASHES (run with --record): " + unrecorded.join(", "));
}

export function installOcrPackages() {
  const done = path.join(npmProject, "node_modules", ".package-lock.json");
  const lock = path.join(npmProject, "package-lock.json");
  if (fs.existsSync(done) && fs.statSync(done).mtimeMs >= fs.statSync(lock).mtimeMs) return;
  const npm = process.platform === "win32" ? "npm.cmd" : "npm";
  const result = spawnSync(npm, ["ci", "--ignore-scripts", "--no-audit", "--no-fund"], {
    cwd: npmProject,
    stdio: "inherit",
    shell: process.platform === "win32",
    windowsHide: true,
  });
  if (result.status !== 0) throw Error("AI_PACK_NPM_CI_FAILED");
}

/** Copies models to `ai/models/<owner>/<repo>/…` and the OCR packages into `node_modules`. */
export async function stage(packageDir) {
  for (const model of pins().models) {
    for (const [file, expected] of Object.entries(model.files)) {
      const source = cached(model, file);
      if (!fs.existsSync(source) || (await sha256(source)) !== expected) throw Error(`AI_CACHE_STALE ${model.repo}/${file}`);
      const target = path.join(packageDir, "ai", "models", model.repo, file);
      fs.mkdirSync(path.dirname(target), { recursive: true });
      fs.copyFileSync(source, target);
    }
  }
  for (const name of OCR_PACKAGES) {
    const target = path.join(packageDir, "node_modules", name);
    if (fs.existsSync(target)) continue; // the backend already ships it
    fs.cpSync(path.join(npmProject, "node_modules", name), target, { recursive: true, dereference: true });
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [action, arg] = process.argv.slice(2);
  if (action === "fetch") {
    await fetchModels({ record: process.argv.includes("--record") });
    installOcrPackages();
    console.log("AI pack sources ready");
  } else if (action === "stage" && arg) {
    await stage(path.resolve(arg));
    console.log("AI pack staged into " + arg);
  } else {
    throw Error("usage: ai-pack.mjs fetch [--record] | stage <package>");
  }
}
