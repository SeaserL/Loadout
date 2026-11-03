// SPDX-License-Identifier: Apache-2.0
// Derived from the verified phase4 encrypted archive implementation.
import fs from "node:fs";
import path from "node:path";
import {
  randomBytes,
  scryptSync,
  createCipheriv,
  createDecipheriv,
} from "node:crypto";
import { DatabaseSync } from "node:sqlite";
import {
  Instance,
  manifest,
  verifyPackage,
  sha,
  json,
  safeFresh,
  keytar,
  withStopped,
} from "./runtime.mjs";
export async function backup(instance, destination, password) {
  return withStopped(instance, () =>
    backupUnlocked(instance, destination, password),
  );
}
function passwordKey(password, salt) {
  if (
    typeof password !== "string" ||
    password.length < 12 ||
    password.length > 512
  )
    throw Error("PASSWORD_LENGTH");
  return scryptSync(password, salt, 32);
}
async function backupUnlocked(instance, destination, password) {
  instance.assertStopped(true);
  safeFresh(destination);
  if (!fs.existsSync(path.join(instance.dir, "data/storage.sqlite")))
    throw Error("DATABASE_NOT_INITIALIZED");
  const snapshot = new DatabaseSync(
    path.join(instance.dir, "data/storage.sqlite"),
  );
  try {
    const state = snapshot.prepare("PRAGMA wal_checkpoint(TRUNCATE)").get();
    if (state.busy) throw Error("DATABASE_BUSY");
  } finally {
    snapshot.close();
  }
  const db = fs.readFileSync(path.join(instance.dir, "data/storage.sqlite"));
  const mapping = path.join(instance.dir, "migration.json");
  const payload = {
    version: 1,
    baseline: manifest().releaseId,
    upstream: manifest().upstream,
    adapterProfile: manifest().adapterProfile,
    config: json(path.join(instance.dir, "instance.json")).config,
    db: db.toString("base64"),
    dbHash: sha(db),
    migration: fs.existsSync(mapping) ? json(mapping) : null,
    vault: await keytar.findCredentials(instance.meta.service),
    bootstrap: await keytar.findCredentials(instance.meta.bootstrap),
  };
  const salt = randomBytes(16),
    iv = randomBytes(12),
    cipher = createCipheriv("aes-256-gcm", passwordKey(password, salt), iv);
  cipher.setAAD(Buffer.from("loadout-fusion-backup-v1"));
  const ciphertext = Buffer.concat([
    cipher.update(JSON.stringify(payload), "utf8"),
    cipher.final(),
  ]);
  fs.writeFileSync(
    destination,
    JSON.stringify({
      version: 1,
      salt: salt.toString("base64"),
      iv: iv.toString("base64"),
      tag: cipher.getAuthTag().toString("base64"),
      ciphertext: ciphertext.toString("base64"),
    }),
    { flag: "wx" },
  );
}
export async function restore(archive, destination, password) {
  return restoreArchive(archive, destination, password);
}
// A forward-only, explicitly tested pair. Normal restore remains exact-release.
// Rollback uses the old package and its pre-upgrade archive in a fresh directory.
export async function upgrade(archive, destination, password, sourceManifest) {
  const target = manifest();
  // Builds of one profile share the OmniRoute base and the schema; every 1.0.x release is 067.1.
  const sameProfile = /^fusion-v3\.8\.52-isolated-067\.1-[a-f0-9]{12}$/.test(sourceManifest?.releaseId ?? "") &&
    sourceManifest.upstream === target.upstream && target.upstream === "ad6f33f66bda34eba98645d3bfdabbde294bc843" &&
    sourceManifest.adapterProfile === "067.1" && target.adapterProfile === "067.1";
  // Same OmniRoute base, same schema: the isolated data moves forward as it is.
  const desktopUpgrade = ["fusion-v3.8.52-isolated-064.1-c03260835d4c", "fusion-v3.8.52-isolated-066.1-cec9977a9720"].includes(sourceManifest?.releaseId) &&
    sourceManifest.upstream === target.upstream && target.upstream === "ad6f33f66bda34eba98645d3bfdabbde294bc843" &&
    ["064.1", "066.1"].includes(sourceManifest.adapterProfile) && target.adapterProfile === "067.1";
  const consoleUpgrade = sourceManifest?.releaseId === "fusion-v3.8.52-isolated-058.1-39f5ab1fafa9" &&
    sourceManifest.upstream === target.upstream && target.upstream === "ad6f33f66bda34eba98645d3bfdabbde294bc843" &&
    sourceManifest.adapterProfile === "058.1" && target.adapterProfile === "064.1";
  const releaseConsolidation = sourceManifest?.releaseId === "fusion-v3.8.52-isolated-057.1-df64cd42e4fa" &&
    sourceManifest.upstream === target.upstream && target.upstream === "ad6f33f66bda34eba98645d3bfdabbde294bc843" &&
    sourceManifest.adapterProfile === "057.1" && target.adapterProfile === "058.1";
  const quotaModelUpgrade = sourceManifest?.releaseId === "fusion-v3.8.52-isolated-056.1-b374e2f3fcdf" &&
    sourceManifest.upstream === target.upstream && target.upstream === "ad6f33f66bda34eba98645d3bfdabbde294bc843" &&
    sourceManifest.adapterProfile === "056.1" && target.adapterProfile === "057.1";
  const pricingUpgrade = sourceManifest?.releaseId === "fusion-v3.8.52-isolated-055.1-4d0b0a9c19c8" &&
    sourceManifest.upstream === target.upstream && target.upstream === "ad6f33f66bda34eba98645d3bfdabbde294bc843" &&
    sourceManifest.adapterProfile === "055.1" && target.adapterProfile === "056.1";
  const radarUpgrade = sourceManifest?.releaseId === "fusion-v3.8.52-isolated-054.1-7f8bb48c76cd" &&
    sourceManifest.upstream === target.upstream && target.upstream === "ad6f33f66bda34eba98645d3bfdabbde294bc843" &&
    sourceManifest.adapterProfile === "054.1" && target.adapterProfile === "055.1";
  const readOnlyUpgrade = sourceManifest?.releaseId === "fusion-v3.8.52-isolated-053.1-5bb0cd9cc642" &&
    sourceManifest.upstream === target.upstream && target.upstream === "ad6f33f66bda34eba98645d3bfdabbde294bc843" &&
    sourceManifest.adapterProfile === "053.1" && target.adapterProfile === "054.1";
  const subscriptionUpgrade = sourceManifest?.upstream === "ad6f33f66bda34eba98645d3bfdabbde294bc843" &&
    target.upstream === sourceManifest.upstream && sourceManifest.adapterProfile === "052.1" &&
    target.adapterProfile === "053.1" && sourceManifest.releaseId === "fusion-v3.8.52-isolated-052.1-eb2d7b7aede2";
  if (
    !sameProfile && !desktopUpgrade && !consoleUpgrade && !releaseConsolidation && !quotaModelUpgrade && !pricingUpgrade && !radarUpgrade && !readOnlyUpgrade && !subscriptionUpgrade && (
    sourceManifest?.upstream !== "c1e30b7676975feb298b49eff6ff58923c04b89e" ||
    target.upstream !== "ad6f33f66bda34eba98645d3bfdabbde294bc843" ||
    sourceManifest.adapterProfile !== "052.1" ||
    target.adapterProfile !== "052.1" ||
    !sourceManifest.releaseId?.startsWith("fusion-v3.8.51-isolated-052.1-") )
  )
    throw Error("UPGRADE_PAIR_UNSUPPORTED");
  return restoreArchive(archive, destination, password, sourceManifest);
}
async function restoreArchive(archive, destination, password, sourceManifest) {
  safeFresh(destination);
  verifyPackage(false);
  const enc = json(archive);
  if (enc.version !== 1) throw Error("BACKUP_VERSION");
  let payload;
  try {
    const dec = createDecipheriv(
      "aes-256-gcm",
      passwordKey(password, Buffer.from(enc.salt, "base64")),
      Buffer.from(enc.iv, "base64"),
    );
    dec.setAAD(Buffer.from("loadout-fusion-backup-v1"));
    dec.setAuthTag(Buffer.from(enc.tag, "base64"));
    payload = JSON.parse(
      Buffer.concat([
        dec.update(Buffer.from(enc.ciphertext, "base64")),
        dec.final(),
      ]),
    );
  } catch {
    throw Error("BACKUP_AUTH_FAILED");
  }
  const db = Buffer.from(payload.db ?? "", "base64");
  if (
    payload.version !== 1 ||
    payload.baseline !== (sourceManifest?.releaseId ?? manifest().releaseId) ||
    (sourceManifest &&
      (payload.upstream !== sourceManifest.upstream ||
        payload.adapterProfile !== sourceManifest.adapterProfile)) ||
    sha(db) !== payload.dbHash ||
    db.subarray(0, 16).toString() !== "SQLite format 3\0" ||
    !Array.isArray(payload.vault) ||
    !Array.isArray(payload.bootstrap)
  )
    throw Error("BACKUP_INVALID");
  for (const e of [...payload.vault, ...payload.bootstrap])
    if (
      typeof e.account !== "string" ||
      typeof e.password !== "string" ||
      e.password.length > 2400
    )
      throw Error("BACKUP_ENTRY_INVALID");
  const runtime = payload.bootstrap.find((e) => e.account === "runtime");
  if (!runtime) throw Error("BOOTSTRAP_INVALID");
  const self = await Instance.create(
    destination,
    JSON.parse(runtime.password),
    payload.config,
  );
  try {
    fs.writeFileSync(path.join(self.dir, "data/storage.sqlite"), db, {
      flag: "wx",
    });
    if (payload.migration)
      fs.writeFileSync(
        path.join(self.dir, "migration.json"),
        JSON.stringify(payload.migration),
      );
    for (const e of payload.vault)
      await keytar.setPassword(self.meta.service, e.account, e.password);
    for (const e of payload.bootstrap)
      await keytar.setPassword(self.meta.bootstrap, e.account, e.password);
    if (sourceManifest)
      fs.writeFileSync(
        path.join(self.dir, "upgrade.json"),
        JSON.stringify({
          from: sourceManifest.releaseId,
          to: manifest().releaseId,
          nativeMigrations: "PENDING_START",
          rollback: "OLD_PACKAGE_PRE_UPGRADE_ARCHIVE",
        }),
        { flag: "wx" },
      );
    return self;
  } catch (e) {
    await self.cleanup();
    fs.writeFileSync(path.join(self.dir, "FAILED"), "RESTORE_FAILED");
    throw e;
  }
}
