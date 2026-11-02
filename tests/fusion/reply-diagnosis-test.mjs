// SPDX-License-Identifier: Apache-2.0
import assert from "node:assert/strict";
import { diagnoseReply } from "../../scripts/fusion/reply-diagnosis.mjs";
const expected = ["MARKER-A", "RECEIPT-A", "37"];
const cases = [
  ["exact", "MARKER-A|RECEIPT-A|37", "EXACT", true],
  ["existing-trim", "\nMARKER-A|RECEIPT-A|37\t", "EXACT", true],
  ["pipe-padding", "MARKER-A | RECEIPT-A | 37", "FORMAT_ONLY", false],
  ["pipe-newlines", "MARKER-A\n|\tRECEIPT-A|\n37", "FORMAT_ONLY", false],
  ["inline-code", "`MARKER-A|RECEIPT-A|37`", "FORMAT_ONLY", false],
  ["fenced-code", "```text\nMARKER-A | RECEIPT-A | 37\n```", "FORMAT_ONLY", false],
  ["wrong-marker", "MARKER-B|RECEIPT-A|37", "CONTENT_MISMATCH", false],
  ["wrong-receipt", "MARKER-A|RECEIPT-B|37", "CONTENT_MISMATCH", false],
  ["wrong-number", "MARKER-A|RECEIPT-A|38", "CONTENT_MISMATCH", false],
  ["numeric-coercion-denied", "MARKER-A|RECEIPT-A|37.0", "CONTENT_MISMATCH", false],
  ["order-denied", "RECEIPT-A|MARKER-A|37", "CONTENT_MISMATCH", false],
  ["case-denied", "marker-a|RECEIPT-A|37", "CONTENT_MISMATCH", false],
  ["missing-field", "MARKER-A|37", "UNRECOGNIZED_STRUCTURE", false],
  ["extra-field", "MARKER-A|RECEIPT-A|37|extra", "UNRECOGNIZED_STRUCTURE", false],
  ["contains-is-not-equality", "Answer: MARKER-A|RECEIPT-A|37", "CONTENT_MISMATCH", false],
  ["extra-explanation", "MARKER-A|RECEIPT-A|37\nThis is my answer.", "CONTENT_MISMATCH", false],
  ["partial-wrapper", "`MARKER-A|RECEIPT-A|37", "CONTENT_MISMATCH", false],
  ["nested-wrapper-denied", "``MARKER-A|RECEIPT-A|37``", "CONTENT_MISMATCH", false],
  ["inside-field-whitespace", "MARKER- A|RECEIPT-A|37", "CONTENT_MISMATCH", false],
  ["unicode-lookalike", "MARKER-A|RECEIPT-A|３７", "CONTENT_MISMATCH", false],
  ["array-not-coerced", ["MARKER-A|RECEIPT-A|37"], "MISSING_TEXT", false],
  ["empty", "", "MISSING_TEXT", false],
];
for (const [name, content, category, strict] of cases) {
  const result = diagnoseReply(content, expected);
  assert.equal(result.classification, category, name);
  assert.equal(result.strictPass, strict, name);
  assert.equal(result.semanticFieldsExact, ["EXACT", "FORMAT_ONLY"].includes(category), name);
  assert(!JSON.stringify(result).includes("MARKER-A"), "RAW_CONTENT_IN_DIAGNOSIS");
}
console.log(JSON.stringify({cases:cases.length,passed:cases.length,strictAcceptanceUnchanged:true}));
