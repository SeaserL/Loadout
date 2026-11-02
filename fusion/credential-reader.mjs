// SPDX-License-Identifier: Apache-2.0
import fs from "node:fs";
import { createHash } from "node:crypto";
import {
  AI_VARIANT,
  WorkBuddyAtRestKeyProvider,
  classifyDesktopAuthDocument,
  openAuthField,
  parseWorkBuddyAuth,
} from "./vendor/connector.mjs";
import { accountDigest } from "./workbuddy-connect.mjs";
import path from "node:path";
export function createReader({
  readDocument,
  readBinding,
  keyProvider,
  now = Date.now,
}) {
  let bound, previousToken;
  const stats = { reads: 0, tokenChanges: 0 };
  return {
    stats,
    async current() {
      const binding = JSON.parse(await readBinding());
      bound ??= binding.account_hash;
      if (!bound || binding.account_hash !== bound)
        throw Error("ACCOUNT_CHANGED");
      let raw, doc;
      try {
        raw = await readDocument();
        doc = JSON.parse(raw);
      } catch {
        throw Error("AUTH_SOURCE_INVALID");
      }
      if (accountDigest(doc.account?.uid) !== bound)
        throw Error("ACCOUNT_CHANGED");
      const c = classifyDesktopAuthDocument(raw);
      if (c.format !== "encrypted") throw Error("ENCRYPTED_SOURCE_REQUIRED");
      const field = c.wrapped.fields.find((f) => f.field === "accessToken");
      if (!field) throw Error("ACCESS_ENVELOPE_MISSING");
      const key = await keyProvider.protectorKeyFor([field.envelope.keyId]);
      doc.auth.accessToken = openAuthField(key, field.envelope);
      delete doc.auth.refreshToken;
      const value = parseWorkBuddyAuth(JSON.stringify(doc));
      if (
        !value?.accessToken ||
        value.expiresAtMs <= now() ||
        !["workbuddy.ai", "www.workbuddy.ai"].includes(value.domain)
      )
        throw Error("UNUSABLE_DESKTOP_SESSION");
      const digest = createHash("sha256")
        .update(value.accessToken)
        .digest("hex");
      if (previousToken && digest !== previousToken) stats.tokenChanges++;
      previousToken = digest;
      stats.reads++;
      return { value, boundAccountHash: bound };
    },
  };
}
export function desktopReader({ authPath, exe, accountHash }) {
  const provider = new WorkBuddyAtRestKeyProvider({
    product: AI_VARIANT.electron,
    electronPath: exe,
    discovery: "none",
    timeoutMs: 10000,
  });
  return createReader({
    readDocument: () => fs.readFileSync(authPath, "utf8"),
    readBinding: () => JSON.stringify({ account_hash: accountHash }),
    keyProvider: provider,
  });
}
