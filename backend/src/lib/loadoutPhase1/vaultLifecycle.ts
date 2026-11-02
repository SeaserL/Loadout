// SPDX-License-Identifier: Apache-2.0
// API-key trial only. Ordinary database backups contain references, never secrets.
import { randomBytes, createCipheriv, createDecipheriv, scryptSync } from "node:crypto";
import * as vault from "./secretStore";

const globals = globalThis as typeof globalThis & { __loadoutWrites?: Promise<unknown> };
function exclusive<T>(action: () => Promise<T>): Promise<T> {
  const task = (globals.__loadoutWrites ?? Promise.resolve()).then(action);
  globals.__loadoutWrites = task.catch(() => {});
  return task;
}
async function referenced(): Promise<Set<string>> {
  const { getDbInstance } = await import("../db/core");
  const refs = new Set<string>();
  const visit = (value: unknown) => {
    if (vault.isReference(value)) refs.add(value as string);
    else if (Array.isArray(value)) value.forEach(visit);
    else if (value && typeof value === "object") Object.values(value).forEach(visit);
  };
  for (const row of getDbInstance().prepare("SELECT * FROM provider_connections").all()) {
    visit(row);
    if (typeof row.provider_specific_data === "string")
      visit(JSON.parse(row.provider_specific_data));
  }
  return refs;
}
async function reconcile() {
  const keep = await referenced();
  if (vault.needsPrune(keep)) await vault.prune(keep);
}
export async function write<T>(operation: () => Promise<T>): Promise<T> {
  if (!vault.enabled()) return operation();
  return exclusive(async () => {
    try {
      return await operation();
    } finally {
      // Both successful writes and failed SQL writes must reconcile OS entries.
      await reconcile();
    }
  });
}
function passwordKey(password: unknown, salt: Buffer) {
  if (typeof password !== "string" || password.length < 12 || password.length > 512)
    throw new Error("INVALID_BACKUP_PASSWORD");
  return scryptSync(password, salt, 32);
}
export async function exportVault(password: unknown) {
  return exclusive(async () => {
    const entries = await vault.entries(await referenced());
    const salt = randomBytes(16),
      iv = randomBytes(12),
      key = passwordKey(password, salt);
    try {
      const cipher = createCipheriv("aes-256-gcm", key, iv);
      cipher.setAAD(Buffer.from("loadout-vault-backup-v1"));
      const ciphertext = Buffer.concat([
        cipher.update(JSON.stringify(entries), "utf8"),
        cipher.final(),
      ]);
      return {
        version: 1,
        salt: salt.toString("base64"),
        iv: iv.toString("base64"),
        tag: cipher.getAuthTag().toString("base64"),
        ciphertext: ciphertext.toString("base64"),
      };
    } finally {
      key.fill(0);
    }
  });
}
export async function restoreVault(password: unknown, backup: Record<string, unknown>) {
  return exclusive(async () => {
    if (!backup || backup.version !== 1) throw new Error("INVALID_BACKUP");
    const decode = (name: string, size?: number) => {
      if (typeof backup[name] !== "string" || (backup[name] as string).length > 1400000)
        throw new Error("INVALID_BACKUP");
      const bytes = Buffer.from(backup[name] as string, "base64");
      if (size && bytes.length !== size) throw new Error("INVALID_BACKUP");
      return bytes;
    };
    const salt = decode("salt", 16),
      iv = decode("iv", 12),
      tag = decode("tag", 16);
    const key = passwordKey(password, salt);
    let entries: unknown;
    try {
      const cipher = createDecipheriv("aes-256-gcm", key, iv);
      cipher.setAAD(Buffer.from("loadout-vault-backup-v1"));
      cipher.setAuthTag(tag);
      entries = JSON.parse(
        Buffer.concat([cipher.update(decode("ciphertext")), cipher.final()]).toString("utf8")
      );
    } catch {
      throw new Error("BACKUP_AUTHENTICATION_FAILED");
    } finally {
      key.fill(0);
    }
    // Validate every entry and conflict before the first OS-store mutation.
    if (
      !Array.isArray(entries) ||
      entries.length > 10000 ||
      entries.some(
        (e) =>
          !e ||
          !/^loadout-ref:v1:[a-f0-9-]{36}$/.test(e.account) ||
          typeof e.password !== "string" ||
          e.password.length === 0 ||
          Buffer.byteLength(e.password) > 2400
      )
    )
      throw new Error("INVALID_BACKUP_ENTRIES");
    const unique = new Set(entries.map((e) => e.account));
    if (unique.size !== entries.length) throw new Error("DUPLICATE_BACKUP_REF");
    const keep = await referenced();
    await vault.restoreEntries(entries.filter((e) => keep.has(e.account)));
    await reconcile();
    return { restored: entries.filter((e) => keep.has(e.account)).length };
  });
}
