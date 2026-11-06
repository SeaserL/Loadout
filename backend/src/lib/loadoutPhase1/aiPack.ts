// SPDX-License-Identifier: Apache-2.0
// Which 1.1 edition this package is, and whether the AI pack (local AI runtime and models) is
// installed in it (ExecPlan 071). The backend runs with the package folder as its working directory.
import fs from "node:fs";
import path from "node:path";

const packageRoot = () => process.cwd();

function readJson(file: string): Record<string, any> | null {
  try {
    return JSON.parse(fs.readFileSync(file, "utf8"));
  } catch {
    return null;
  }
}

export function aiPackState() {
  const manifest = readJson(path.join(packageRoot(), "fusion-manifest.json")) ?? {};
  const pack = readJson(path.join(packageRoot(), "ai-pack.json"));
  return {
    edition: (manifest.edition === "lite" ? "lite" : "full") as "lite" | "full",
    installed: !!pack,
    version: (pack?.packVersion as string | undefined) ?? null,
    required: (manifest.aiPack as string | undefined) ?? null,
  };
}

/** Models live at `ai/models/<owner>/<repo>/…`, the layout transformers.js expects locally. */
export const packModelPath = () => path.join(packageRoot(), "ai", "models");
