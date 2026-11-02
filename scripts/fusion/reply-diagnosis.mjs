// SPDX-License-Identifier: Apache-2.0
// Test-only classification. Never upgrades the existing strict acceptance gate.
export function diagnoseReply(content, expected) {
  if (!Array.isArray(expected) || expected.length !== 3 || expected.some(v => typeof v !== "string" || !v || v.includes("|")))
    throw Error("INVALID_EXPECTED_REPLY");
  const result = { strictPass: false, semanticFieldsExact: false, classification: "MISSING_TEXT", representation: "plain", fieldCount: 0,
    fields: { marker: false, receipt: false, value: false }, characters: typeof content === "string" ? content.length : 0 };
  if (typeof content !== "string" || !content.trim()) return result;
  let payload = content.trim();
  result.strictPass = payload === expected.join("|");
  const fence = /^```(?:text)?\r?\n([^`]*)\r?\n```$/.exec(payload);
  if (fence) { result.representation = "fenced-code"; payload = fence[1]; }
  else if (/^`[^`]*`$/.test(payload)) { result.representation = "inline-code"; payload = payload.slice(1, -1); }
  const parts = payload.split("|");
  result.fieldCount = parts.length;
  if (parts.length !== 3) { result.classification = "UNRECOGNIZED_STRUCTURE"; return result; }
  // Only edge ASCII whitespace may be classified as formatting. No value rewriting.
  const fields = parts.map(v => v.replace(/^[ \t\r\n]+|[ \t\r\n]+$/g, ""));
  result.edgeWhitespace = parts.some((v, i) => v !== fields[i]);
  result.fields = Object.fromEntries(["marker", "receipt", "value"].map((name, i) => [name, fields[i] === expected[i]]));
  result.semanticFieldsExact = Object.values(result.fields).every(Boolean);
  result.classification = result.strictPass ? "EXACT" : result.semanticFieldsExact ? "FORMAT_ONLY" : "CONTENT_MISMATCH";
  return result;
}
