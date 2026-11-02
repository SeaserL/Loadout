// SPDX-License-Identifier: Apache-2.0
import fs from "node:fs";
import path from "node:path";
import { createHash } from "node:crypto";
import { chatHeaders, chatUserAgent } from "./vendor/connector.mjs";
export const endpoint = "https://www.workbuddy.ai/v2/chat/completions";
export const accountDigest = (uid) =>
  createHash("sha256")
    .update("loadout-workbuddy-account\0" + uid)
    .digest("hex");
export function wireHeaders(value, installed) {
  return {
    ...chatHeaders(
      value,
      chatUserAgent(installed, "global"),
      installed.clientVersion,
    ),
    Authorization: "Bearer " + value.accessToken,
  };
}
export function errorSummary(status, text) {
  let data;
  try {
    data = JSON.parse(text);
  } catch {
    return { status, category: "NON_JSON" };
  }
  const msg = String(data.msg ?? data.message ?? data.error?.message ?? "");
  return {
    status,
    applicationCode: typeof data.code === "number" ? data.code : undefined,
    category: /conversation.*missing/i.test(msg)
      ? "MISSING_CONVERSATION"
      : /first message.*system/i.test(msg)
        ? "SYSTEM_FIRST_REQUIRED"
        : /unapproved|channel/i.test(msg)
          ? "CHANNEL_REJECTED"
          : /token|expired|auth/i.test(msg)
            ? "AUTH"
            : /limit|quota|credit/i.test(msg)
              ? "LIMIT"
              : "UNCLASSIFIED",
  };
}

// The chat User-Agent must name what is installed; an update must not leave stale versions behind.
export function installedVersions(exe) {
  try {
    const resources = path.join(path.dirname(exe), "resources");
    const read = (file) => JSON.parse(fs.readFileSync(path.join(resources, file), "utf8"));
    const clientVersion = read("install-manifest.json").appVersion;
    const cliVersion = read("app.asar.unpacked/cli/package.json").version;
    if (/^\d+(\.\d+){2,3}$/.test(clientVersion) && /^\d+(\.\d+){2}(-[\w.]+)?$/.test(cliVersion))
      return { clientVersion, cliVersion };
  } catch {
    // Keep what the instance recorded.
  }
  return {};
}
