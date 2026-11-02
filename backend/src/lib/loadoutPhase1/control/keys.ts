// SPDX-License-Identifier: Apache-2.0
// Encrypted export / import of every API-key account. Same `loadout-keys` envelope as
// crates/core/src/keys_export.rs, so files exported by the Rust release import here unchanged.
import fs from "node:fs";
import path from "node:path";
import { createCipheriv, createDecipheriv, pbkdf2Sync, randomBytes } from "node:crypto";
import { accountAdd, accountKey, providerAdd, readAccountSecret } from "./accounts";
import { KINDS, kindOf } from "./kinds";
import { invalid, isSubscription, kvGet, snapshot, type Native } from "./shared";

const FORMAT = "loadout-keys";
const KDF = "pbkdf2-hmac-sha256";
const CIPHER = "chacha20-poly1305";
const ITERATIONS = 600_000;
const MIN_PASSPHRASE = 8;
const BAD_FILE = "口令不对，或者文件已损坏、被改动";

type Envelope = {
  format: string;
  version: number;
  kdf: string;
  iterations: number;
  salt: string;
  cipher: string;
  nonce: string;
  ciphertext: string;
};
const aad = (e: Envelope) =>
  Buffer.from([e.format, e.version, e.kdf, e.iterations, e.salt, e.cipher, e.nonce].join("\n"));
const keyFor = (passphrase: string, salt: Buffer, iterations: number) =>
  pbkdf2Sync(passphrase, salt, iterations, 32, "sha256");

export function seal(plain: Buffer, passphrase: string, iterations = ITERATIONS): Envelope {
  const salt = randomBytes(16);
  const nonce = randomBytes(12);
  const envelope: Envelope = {
    format: FORMAT,
    version: 1,
    kdf: KDF,
    iterations,
    salt: salt.toString("base64"),
    cipher: CIPHER,
    nonce: nonce.toString("base64"),
    ciphertext: "",
  };
  const key = keyFor(passphrase, salt, iterations);
  const cipher = createCipheriv(CIPHER, key, nonce, { authTagLength: 16 });
  cipher.setAAD(aad(envelope), { plaintextLength: plain.length });
  const body = Buffer.concat([cipher.update(plain), cipher.final(), cipher.getAuthTag()]);
  key.fill(0);
  envelope.ciphertext = body.toString("base64");
  return envelope;
}

export function open(text: string, passphrase: string): Buffer {
  let e: Envelope;
  try {
    e = JSON.parse(text);
  } catch {
    throw invalid("这不是 Loadout 导出的密钥文件（.loadout-keys）");
  }
  if (e.format !== FORMAT || e.version > 1) throw invalid("不支持的文件版本");
  if (
    e.kdf !== KDF ||
    e.cipher !== CIPHER ||
    !(e.iterations >= 100_000 && e.iterations <= 10_000_000)
  )
    throw invalid(BAD_FILE);
  const key = keyFor(passphrase, Buffer.from(e.salt, "base64"), e.iterations);
  try {
    const raw = Buffer.from(e.ciphertext, "base64");
    const decipher = createDecipheriv(CIPHER, key, Buffer.from(e.nonce, "base64"), {
      authTagLength: 16,
    });
    const body = raw.subarray(0, raw.length - 16);
    decipher.setAAD(aad(e), { plaintextLength: body.length });
    decipher.setAuthTag(raw.subarray(raw.length - 16));
    return Buffer.concat([decipher.update(body), decipher.final()]);
  } catch {
    throw invalid(BAD_FILE);
  } finally {
    key.fill(0);
  }
}

function checkPassphrase(value: unknown): string {
  if (typeof value !== "string" || [...value].length < MIN_PASSPHRASE)
    throw invalid("口令至少 8 个字符");
  return value;
}

function defaultPath() {
  const home = process.env.LOADOUT_USER_HOME;
  const dir = home && fs.existsSync(path.join(home, "Desktop")) ? path.join(home, "Desktop") : (home ?? process.cwd());
  const stamp = new Date().toISOString().replace(/[-:]/g, "").slice(0, 15);
  return path.join(dir, `loadout-keys-${stamp}.loadout-keys`);
}

export async function exportKeys(p: any) {
  if (p?.confirm !== "EXPORT") throw invalid("输入 EXPORT 确认");
  const passphrase = checkPassphrase(p?.passphrase);
  const providers = await snapshot();
  const manual = kvGet<any[]>("manualModels", []);
  const skipped: string[] = [];
  let accounts = 0;
  const bundle = {
    exported_at_ms: Date.now(),
    loadout_version: "fusion",
    providers: [] as any[],
  };
  for (const provider of providers) {
    // Sign-ins and local bridges cannot be shared; only API keys travel.
    if (provider.kind.auth === "external_managed" || provider.kind.auth === "oauth") continue;
    const list: any[] = [];
    for (const conn of provider.conns) {
      if (isSubscription(conn)) continue;
      const key = await readAccountSecret(conn.id);
      if (!key) {
        skipped.push(`${provider.slug}/${conn.name}`);
        continue;
      }
      list.push({
        alias: conn.name,
        display_name: conn.displayName ?? null,
        api_key: key,
        manual_models: manual
          .filter((m) => m.provider === provider.slug && m.account === conn.name)
          .map((m) => ({
            upstream_model_id: m.model,
            protocols: m.protocols,
            context_window: m.context_window,
          })),
      });
    }
    if (!list.length) continue;
    accounts += list.length;
    bundle.providers.push({
      slug: provider.slug,
      kind: provider.kind.kind,
      display_name: provider.display,
      base_url: provider.baseUrl,
      settings: provider.settings ?? {},
      accounts: list,
    });
  }
  const plain = Buffer.from(JSON.stringify(bundle));
  const envelope = seal(plain, passphrase);
  plain.fill(0);
  const target = String(p?.path ?? "").trim() || defaultPath();
  if (!path.isAbsolute(target)) throw invalid("保存路径必须是完整路径");
  fs.writeFileSync(target, JSON.stringify(envelope, null, 2), { flag: "wx" });
  return { path: target, providers: bundle.providers.length, accounts, skipped };
}

export async function importKeys(native: Native, p: any) {
  const passphrase = checkPassphrase(p?.passphrase);
  const text =
    typeof p?.content === "string" && p.content
      ? p.content
      : (() => {
          const file = String(p?.path ?? "").trim();
          if (!file) throw invalid("选择要导入的文件");
          try {
            return fs.readFileSync(file, "utf8");
          } catch {
            throw invalid(`读不到文件 ${file}`);
          }
        })();
  const plain = open(text, passphrase);
  let bundle: any;
  try {
    bundle = JSON.parse(plain.toString("utf8"));
  } catch {
    throw invalid("解密后的内容无效");
  } finally {
    plain.fill(0);
  }
  const report = {
    providers: 0,
    accounts: 0,
    keys_updated: 0,
    skipped: [] as string[],
    sync_errors: [] as string[],
  };
  for (const provider of bundle.providers ?? []) {
    let existing = (await snapshot()).find((x) => x.slug === provider.slug);
    if (!existing) {
      // Older exports name platform kinds this build may not know; a base URL is enough.
      const kind = KINDS.find((k) => k.kind === provider.kind);
      const useKind =
        kind && (kind.fixed_slug === provider.slug || !kind.fixed_slug) ? kind.kind : "openai-compatible";
      const base = provider.base_url ?? kindOf(provider.kind, null).default_base_url;
      if (!base) {
        report.skipped.push(`${provider.slug}（缺少 Base URL）`);
        continue;
      }
      try {
        await providerAdd(native, {
          slug: provider.slug,
          kind: useKind,
          base_url: base,
          display_name: provider.display_name,
          settings: provider.settings,
        });
        report.providers += 1;
      } catch (e) {
        report.skipped.push(`${provider.slug}：${(e as Error).message}`);
        continue;
      }
      existing = (await snapshot()).find((x) => x.slug === provider.slug);
    }
    for (const account of provider.accounts ?? []) {
      const name = `${provider.slug}/${account.alias}`;
      if (existing?.conns.some((c) => c.name === account.alias)) {
        if (p?.overwrite === true) {
          try {
            await accountKey(native, provider.slug, account.alias, account.api_key);
            report.keys_updated += 1;
          } catch (e) {
            report.skipped.push(`${name}：${(e as Error).message}`);
          }
        } else report.skipped.push(`${name}（已存在）`);
        continue;
      }
      try {
        // Import is local; models are synced from the account card when the user asks.
        await accountAdd(native, {
          provider: provider.slug,
          alias: account.alias,
          api_key: account.api_key,
          display_name: account.display_name,
          sync_models: false,
        });
        report.accounts += 1;
      } catch (e) {
        report.skipped.push(`${name}：${(e as Error).message}`);
      }
    }
  }
  return report;
}
