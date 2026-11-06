// SPDX-License-Identifier: Apache-2.0
// What goes into each 1.1 edition (ExecPlan 071). Paths are package-relative with forward slashes.
// One assembled folder holds everything: the app (the lite edition) plus the AI pack. The full
// edition is both; `ai-pack.json` at the package root lists the pack's files and marks a full install.

export const PACK_FILE = "ai-pack.json";

/** Shipped by the upstream build but used by neither edition. */
const UNUSED = [
  /^docs\//, // upstream documentation; only the docs search route read it
  /^\.build\/loadout-fusion\/server\/app\/docs\//, // that route
  /^public\/sponsors\//,
  /\.map$/,
  // build tools that the standalone trace picked up
  /(^|\/)node_modules\/(esbuild|@esbuild\/[^/]+|tsx|babel-plugin-react-compiler|@types\/node)\//,
  // last-resort WASM database; the package always has better-sqlite3 and node:sqlite
  /(^|\/)node_modules\/sql\.js\//,
  // browser automation for the ChatGPT web provider, which Loadout does not offer
  /(^|\/)node_modules\/(playwright-core|playwright)\//,
  // tunnels are not offered
  /(^|\/)node_modules\/@ngrok\//,
  // the AI runtime is Windows x64 only
  /(^|\/)node_modules\/onnxruntime-node\/bin\/napi-v\d+\/(linux|darwin|win32\/arm64)\//,
  // onnxruntime-web is imported by transformers.js but never runs in Node: keep its JS, drop WASM
  /(^|\/)node_modules\/onnxruntime-web\/dist\/.*\.wasm$/,
];

/** The local AI runtime, the OCR packages and the models: the AI pack. */
const AI_PACK = [
  /(^|\/)node_modules\/(onnxruntime-node|onnxruntime-web|onnxruntime-common)\//,
  /(^|\/)node_modules\/@huggingface\/(transformers|tokenizers|jinja)\//,
  /(^|\/)node_modules\/@atjsh\/llmlingua-2\//,
  /(^|\/)node_modules\/js-tiktoken\//,
  /(^|\/)node_modules\/(omniglyph|gpt-tokenizer)\//,
  /(^|\/)node_modules\/(@gutenye\/[^/]+|@techstark\/opencv-js|js-clipper)\//,
  /^open-sse\/services\/compression\/engines\/llmlingua\/(onnxWorker\.js|package\.json)$/,
  /^ai\//,
];

export const unused = (file) => UNUSED.some((re) => re.test(file));
export const inAiPack = (file) => file !== PACK_FILE && AI_PACK.some((re) => re.test(file));
