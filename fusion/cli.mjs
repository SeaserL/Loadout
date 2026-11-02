// SPDX-License-Identifier: Apache-2.0
import fs from "node:fs";
import path from "node:path";
import {
  Instance,
  keytar,
  json,
  verifyPackage,
  writeJson,
  validateConfig,
  withStopped,
} from "./runtime.mjs";
import { backup, restore, upgrade } from "./backup.mjs";
import { snapshot, preview, migrate } from "./migrate.mjs";
import { preflight } from "./preflight.mjs";
import { configure } from "./policy.mjs";
import { fingerprint, compile, applyReady, nativeWait } from "./mapping.mjs";

const [command, ...args] = process.argv.slice(2);
const stdinJson = () => {
  if (process.stdin.isTTY) throw Error("PRIVATE_STDIN_REQUIRED");
  const raw = fs.readFileSync(0, "utf8");
  if (raw.length > 2 * 1024 * 1024) throw Error("INPUT_TOO_LARGE");
  return JSON.parse(raw);
};
async function password() {
  if (args.includes("--password-stdin")) {
    if (process.stdin.isTTY) throw Error("PRIVATE_STDIN_REQUIRED");
    return fs.readFileSync(0, "utf8").replace(/\r?\n$/, "");
  }
  if (!process.stdin.isTTY) throw Error("PASSWORD_STDIN_FLAG_REQUIRED");
  process.stderr.write("Backup passphrase (12+ characters): ");
  process.stdin.setRawMode(true);
  process.stdin.resume();
  return new Promise((resolve, reject) => {
    let value = "";
    const finish = (error) => {
      process.stdin.off("data", onData);
      process.stdin.setRawMode(false);
      process.stdin.pause();
      process.stderr.write("\n");
      error ? reject(error) : resolve(value);
    };
    function onData(chunk) {
      for (const char of chunk.toString("utf8")) {
        if (char === "\u0003") return finish(Error("CANCELLED"));
        if (char === "\r" || char === "\n") return finish();
        if (char === "\u007f" || char === "\b") value = value.slice(0, -1);
        else if (value.length < 512) value += char;
      }
    }
    process.stdin.on("data", onData);
  });
}
function redact(value) {
  if (Array.isArray(value)) return value.map(redact);
  if (value && typeof value === "object")
    return Object.fromEntries(
      Object.entries(value).map(([key, v]) => [
        key,
        /^(key|apiKey|accessToken|refreshToken|idToken|password|secret|token)$/i.test(
          key,
        )
          ? "[REDACTED]"
          : redact(v),
      ]),
    );
  return value;
}
let result;
try {
  if (command === "preflight") {
    result = preflight(args[0]);
  } else if (command === "verify") {
    const m = verifyPackage();
    result = { release: m.releaseId, files: m.files.length };
  } else if (command === "init") {
    const i = await Instance.create(args[0]);
    result = { created: i.dir, network: "loopback-only" };
  } else if (command === "console") {
    const i = new Instance(args[0]);
    const state = await i.status();
    if (state.status !== "ready") throw Error("START_INSTANCE_FIRST");
    result = { url: `http://127.0.0.1:${state.port}/loadout/index.html` };
  } else if (command === "management-password") {
    // Explicit local reveal, matching client-key --show. Never part of status/logs.
    if (!args.includes("--show")) throw Error("EXPLICIT_SECRET_REVEAL_REQUIRED");
    const i = new Instance(args[0]);
    process.stdout.write((await i.secrets()).password + "\n");
  } else if (["start", "stop", "status"].includes(command)) {
    const i = new Instance(args[0]);
    result =
      command === "start"
        ? await i.start()
        : command === "status"
          ? await i.status()
          : (await i.stop(), { status: "stopped" });
  } else if (command === "configure") {
    const i = new Instance(args[0]),
      config = validateConfig(stdinJson());
    await withStopped(i, async () => {
      i.meta.config = config;
      writeJson(path.join(i.dir, "instance.json"), i.meta);
    });
    result = { configured: true, network: config.network };
  } else if (command === "subscription-scan") {
    const { scanSubscription } = await import("./subscription-scan.mjs");
    result = await scanSubscription(args.length ? { home: args[0], ...(args[1] ? { executable: args[1] } : {}) } : {});
  } else if (command === "subscription-login") {
    const i = new Instance(args[0]), input = stdinJson();
    const baseUrl = input.baseUrl ?? "https://chatgpt.com/backend-api/codex";
    const url = new URL(baseUrl);
    if (!(url.protocol === "http:" && url.hostname === "127.0.0.1") &&
        !(url.origin === "https://chatgpt.com" && url.pathname === "/backend-api/codex" &&
          i.meta.config.network === "allowlist" && i.meta.config.hosts?.includes("chatgpt.com") && i.meta.config.hosts?.includes("auth.openai.com")))
      throw Error("SUBSCRIPTION_NETWORK_REQUIRED");
    const nodes = await i.admin("/api/provider-nodes");
    let node = nodes.nodes.find((n) => n.name === "loadout-codex-subscription" && n.baseUrl === baseUrl && n.apiType === "responses");
    if (!node) node = (await i.admin("/api/provider-nodes", "POST", { type: "openai-compatible", name: "loadout-codex-subscription", prefix: "codex-sub", apiType: "responses", baseUrl })).node;
    result = await i.admin("/api/loadout/subscription", "POST", { operation: "start", provider: node.id, alias: input.alias, authBase: input.authBase });
  } else if (command === "subscription") {
    const i = new Instance(args[0]);
    result = await i.admin("/api/loadout/subscription", "POST", stdinJson());
  } else if (command === "add-workbuddy") {
    const i = new Instance(args[0]),
      wb = i.meta.config.workbuddy;
    if (!wb) throw Error("WORKBUDDY_CONFIG_REQUIRED");
    const secrets = await i.secrets();
    result = redact(
      await i.addProvider("workbuddy", `http://127.0.0.1:${wb.port}/v1`, [
        { alias: "bound", key: secrets.bridge },
      ]),
    );
  } else if (command === "traffic") {
    const i = new Instance(args[0]),
      input = stdinJson();
    if (!Array.isArray(input.connections) || !input.connections.length)
      throw Error("CONNECTIONS_REQUIRED");
    result = await configure(i, input.connections);
  } else if (command === "admin") {
    const i = new Instance(args[0]);
    const method = args[1],
      url = args[2];
    const body =
      method === "GET" || method === "DELETE" ? undefined : stdinJson();
    const response = await i.admin(url, method, body);
    if (url === "/api/keys" && method === "POST" && response.key)
      await keytar.setPassword(
        i.meta.bootstrap,
        "client-" + response.id,
        response.key,
      );
    result = redact(response);
  } else if (command === "add-provider") {
    const i = new Instance(args[0]),
      input = stdinJson();
    result = redact(
      await i.addProvider(
        input.slug,
        input.baseUrl,
        input.accounts,
        input.apiType,
      ),
    );
  } else if (command === "client-key") {
    const i = new Instance(args[0]);
    if (!args.includes("--show")) throw Error("EXPLICIT_SHOW_REQUIRED");
    const value = await keytar.getPassword(i.meta.bootstrap, args[1]);
    if (!value || !/^(client-|route-)/.test(args[1]))
      throw Error("CLIENT_KEY_UNAVAILABLE");
    process.stdout.write(value + "\n");
  } else if (command === "preview") {
    const data = preview(
      args[0],
      args[1] ? json(args[1]) : {},
      args[2] ? json(args[2]) : {},
    );
    result = { ...data, sourceFingerprint: fingerprint(data.source) };
  } else if (command === "migrate") {
    const r = await migrate(args[0], args[1], stdinJson());
    result = { created: r.instance.dir, routes: r.routes };
  } else if (command === "map") {
    const i = new Instance(args[0]),
      source = snapshot(args[1]),
      bindings = json(args[2]),
      models = json(args[3]);
    const migrationIndex = args.indexOf("--aliases");
    const migration = migrationIndex >= 0 ? { aliases: json(args[migrationIndex + 1]) } : {};
    const plan = compile(source, bindings, models, "native-bounded-v1", migration);
    if (args.includes("--apply")) {
      if (!plan.activationReady) throw Error("BLOCKED_MAPPING");
      await i.admin("/api/resilience", "PATCH", nativeWait());
      await applyReady(i, plan, { source, bindings });
      writeJson(path.join(i.dir, "migration.json"), { plan, bindings });
    }
    result = plan;
  } else if (command === "backup") {
    await backup(new Instance(args[0]), args[1], await password());
    result = { archive: args[1] };
  } else if (command === "upgrade") {
    const i = await upgrade(args[0], args[1], await password(), json(args[2]));
    result = {
      created: i.dir,
      nativeMigrations: "PENDING_START",
      originalUntouched: true,
    };
  } else if (command === "restore" || command === "rollback") {
    const i = await restore(args[0], args[1], await password());
    result = { created: i.dir, originalUntouched: true };
  } else if (command === "cleanup") {
    if (!args.includes("--delete-test-credentials"))
      throw Error("EXPLICIT_CREDENTIAL_DELETE_REQUIRED");
    result = { remaining: await new Instance(args[0]).cleanup() };
  } else
    result = {
      usage: [
        "verify",
        "init INSTANCE",
        "start|stop|status INSTANCE",
        "console INSTANCE",
        "management-password INSTANCE --show",
        "configure INSTANCE < config-stdin",
        "add-workbuddy INSTANCE",
        "subscription-scan [CODEX_HOME EXECUTABLE]",
        "subscription-login INSTANCE < {alias,baseUrl?,authBase?}",
        "subscription INSTANCE < {operation:start|status|cancel|list|transport|refresh,...}",
        "traffic INSTANCE < {connections:[IDS]}",
        "admin INSTANCE METHOD /api/path < private-input",
        "add-provider INSTANCE < private-input",
        "client-key INSTANCE client-NAME --show",
        "preview SOURCE_DB [BINDINGS_JSON ENDPOINT_MODELS_JSON]",
        "preflight SOURCE_DB",
        "migrate SOURCE_DB NEW_INSTANCE < private-input",
        "map INSTANCE SOURCE_DB BINDINGS_JSON ENDPOINT_MODELS_JSON [--aliases ALIASES_JSON] [--apply]",
        "backup INSTANCE ARCHIVE [--password-stdin]",
        "restore|rollback ARCHIVE NEW_INSTANCE [--password-stdin]",
        "upgrade ARCHIVE NEW_INSTANCE SOURCE_MANIFEST [--password-stdin]",
        "cleanup INSTANCE --delete-test-credentials",
      ],
      restriction:
        "Instance paths must contain loadout-fusion-isolated. Network defaults to loopback-only; external accounts remain unverified.",
    };
  if (result !== undefined) console.log(JSON.stringify(result, null, 2));
} catch (error) {
  console.error(
    /^[A-Z][A-Z_0-9]+$/.test(error.message)
      ? error.message
      : "OPERATION_FAILED",
  );
  process.exitCode = 1;
}
