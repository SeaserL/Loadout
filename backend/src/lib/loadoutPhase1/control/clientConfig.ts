// SPDX-License-Identifier: Apache-2.0
// One-click client setup (ExecPlan 071). Loadout keeps only a hash of each client key, so a tool
// can only be configured at the moment its key is created: this makes a new key named after the
// tool and writes it into that tool's own config file, after copying the file to a backup. Only
// Loadout's entries are touched; a tool's other settings and its own sign-in stay as they were.
import fs from "node:fs";
import path from "node:path";
import { parse as parseToml, stringify as stringifyToml } from "smol-toml";
import { clientCreate, clientInfos, clientMcp } from "./clients";
import { instanceDir, invalid, type Native } from "./shared";

type Context = { root: string; key: string; model: string; mcp: boolean };
type Tool = {
  id: string;
  name: string;
  file: (home: string) => string;
  /** Where the tool keeps its settings; present means installed. */
  dir: (home: string) => string;
  write: (file: string, ctx: Context) => void;
  /** How to use it afterwards, when that is not automatic. */
  usage?: string;
};

const home = () => {
  const dir = process.env.LOADOUT_USER_HOME;
  if (!dir) throw invalid("不知道当前用户的主目录");
  return dir;
};
const gatewayRoot = () => `http://127.0.0.1:${process.env.PORT}`;

function readJson(file: string): Record<string, any> {
  if (!fs.existsSync(file)) return {};
  const text = fs.readFileSync(file, "utf8").replace(/^﻿/, "");
  if (!text.trim()) return {};
  try {
    const value = JSON.parse(text);
    return value && typeof value === "object" && !Array.isArray(value) ? value : {};
  } catch {
    throw invalid(`${file} 不是标准 JSON（可能带注释），没有改动它`);
  }
}
function writeAtomic(file: string, text: string) {
  fs.mkdirSync(path.dirname(file), { recursive: true });
  const temporary = `${file}.loadout-${process.pid}.tmp`;
  fs.writeFileSync(temporary, text);
  fs.renameSync(temporary, file);
}

const TOOLS: Tool[] = [
  {
    id: "claude-code",
    name: "Claude Code",
    dir: (h) => path.join(h, ".claude"),
    file: (h) => path.join(h, ".claude", "settings.json"),
    write: (file, ctx) => {
      const settings = readJson(file);
      settings.env = {
        ...(settings.env ?? {}),
        ANTHROPIC_BASE_URL: ctx.root,
        ANTHROPIC_AUTH_TOKEN: ctx.key,
        // Background tasks ask for Haiku by name; send them to the same model.
        ANTHROPIC_MODEL: ctx.model,
        ANTHROPIC_DEFAULT_OPUS_MODEL: ctx.model,
        ANTHROPIC_DEFAULT_SONNET_MODEL: ctx.model,
        ANTHROPIC_DEFAULT_HAIKU_MODEL: ctx.model,
      };
      writeAtomic(file, JSON.stringify(settings, null, 2) + "\n");
      if (ctx.mcp) {
        // User-scope MCP servers live in ~/.claude.json.
        const userFile = path.join(path.dirname(path.dirname(file)), ".claude.json");
        const user = readJson(userFile);
        user.mcpServers = {
          ...(user.mcpServers ?? {}),
          loadout: { type: "http", url: `${ctx.root}/api/mcp/stream`, headers: { Authorization: `Bearer ${ctx.key}` } },
        };
        backup(userFile, "claude-code-user");
        writeAtomic(userFile, JSON.stringify(user, null, 2) + "\n");
      }
    },
  },
  {
    id: "codex",
    name: "Codex CLI",
    dir: (h) => path.join(h, ".codex"),
    file: (h) => path.join(h, ".codex", "config.toml"),
    // A profile of its own: the default provider and the ChatGPT sign-in are left alone.
    usage: "codex --profile loadout",
    write: (file, ctx) => {
      const config = (fs.existsSync(file) ? parseToml(fs.readFileSync(file, "utf8")) : {}) as Record<string, any>;
      config.model_providers = {
        ...(config.model_providers ?? {}),
        loadout: {
          name: "Loadout",
          base_url: `${ctx.root}/v1`,
          wire_api: "responses",
          experimental_bearer_token: ctx.key,
        },
      };
      config.profiles = { ...(config.profiles ?? {}), loadout: { model_provider: "loadout", model: ctx.model } };
      if (ctx.mcp) {
        config.mcp_servers = {
          ...(config.mcp_servers ?? {}),
          loadout: { url: `${ctx.root}/api/mcp/stream`, http_headers: { Authorization: `Bearer ${ctx.key}` } },
        };
      }
      writeAtomic(file, stringifyToml(config) + "\n");
    },
  },
  {
    id: "opencode",
    name: "OpenCode",
    dir: (h) => path.join(h, ".config", "opencode"),
    file: (h) => path.join(h, ".config", "opencode", "opencode.json"),
    write: (file, ctx) => {
      if (fs.existsSync(file.replace(/\.json$/, ".jsonc"))) throw invalid("OpenCode 用的是 opencode.jsonc（带注释），没有改动它");
      const config = readJson(file);
      config.$schema ??= "https://opencode.ai/config.json";
      config.provider = {
        ...(config.provider ?? {}),
        loadout: {
          npm: "@ai-sdk/openai-compatible",
          name: "Loadout",
          options: { baseURL: `${ctx.root}/v1`, apiKey: ctx.key },
          models: { [ctx.model]: { name: ctx.model } },
        },
      };
      if (ctx.mcp) {
        config.mcp = {
          ...(config.mcp ?? {}),
          loadout: { type: "remote", url: `${ctx.root}/api/mcp/stream`, headers: { Authorization: `Bearer ${ctx.key}` } },
        };
      }
      writeAtomic(file, JSON.stringify(config, null, 2) + "\n");
    },
  },
];

/** Copies a config file before it is changed; the last five per tool are kept. */
function backup(file: string, tool: string): string | null {
  if (!fs.existsSync(file)) return null;
  const dir = path.join(instanceDir(), "backups", "client-config");
  fs.mkdirSync(dir, { recursive: true });
  const stamp = new Date().toISOString().replace(/[:.]/g, "-");
  const target = path.join(dir, `${tool}-${stamp}${path.extname(file)}`);
  fs.copyFileSync(file, target);
  const mine = fs
    .readdirSync(dir)
    .filter((name) => name.startsWith(`${tool}-`))
    .sort();
  for (const old of mine.slice(0, Math.max(0, mine.length - 5))) fs.rmSync(path.join(dir, old), { force: true });
  return target;
}

export function clientTools() {
  const h = home();
  return TOOLS.map((t) => ({
    id: t.id,
    name: t.name,
    installed: fs.existsSync(t.dir(h)),
    config_path: t.file(h),
    usage: t.usage ?? null,
  }));
}

export async function connectClientTool(native: Native, toolId: string, model: string, mcp: boolean) {
  const tool = TOOLS.find((t) => t.id === toolId);
  if (!tool) throw invalid(`不支持 ${toolId}`);
  if (!model?.trim()) throw invalid("先选一个模型");
  const file = tool.file(home());
  const taken = new Set((await clientInfos(native)).map((c) => c.name));
  let name = tool.name;
  for (let n = 2; taken.has(name); n++) name = `${tool.name} ${n}`;
  const saved = backup(file, tool.id);
  const { client, key } = await clientCreate(native, name);
  if (mcp) await clientMcp(native, name, true);
  try {
    tool.write(file, { root: gatewayRoot(), key, model: model.trim(), mcp });
  } catch (error) {
    // Nothing was written that needs the key; do not leave an unused key behind.
    await native(`/api/keys/${client.id}`, "DELETE").catch(() => {});
    throw error;
  }
  return { client: { ...client, mcp }, tool: tool.id, written: file, backup: saved, usage: tool.usage ?? null };
}
