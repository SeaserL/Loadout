// SPDX-License-Identifier: Apache-2.0
import assert from "node:assert/strict";
import fs from "node:fs";
import { load } from "./harness.mjs";
const {
  adaptWorkbuddyFirstLine: adapt,
  CODEX_FIRST_LINE: original,
  WORKBUDDY_FIRST_LINE: replacement,
} = await load("workbuddy-first-line.mjs");
const cases = [];
for (const newline of ["\n", "\r\n", ""])
  for (const array of [false, true]) {
    const text = original + (newline ? newline + "KEEP INSTRUCTIONS" : "");
    const body = {
      model: "deepseek-v4.1-flash",
      messages: [
        {
          role: "system",
          content: array
            ? [
                { type: "text", text },
                { type: "text", text: original },
              ]
            : text,
        },
        { role: "system", content: original },
        { role: "user", content: original },
        { role: "tool", content: original, tool_call_id: "id" },
      ],
      tools: [
        {
          type: "function",
          function: { name: "read", parameters: { description: original } },
        },
      ],
    };
    const copy = structuredClone(body),
      result = adapt(body);
    assert.deepEqual(body, copy);
    assert.notEqual(result, body);
    if (array) {
      assert.equal(
        result.messages[0].content[0].text,
        replacement + (newline ? newline + "KEEP INSTRUCTIONS" : ""),
      );
      result.messages[0].content[0].text = text;
    } else {
      assert.equal(
        result.messages[0].content,
        replacement + (newline ? newline + "KEEP INSTRUCTIONS" : ""),
      );
      result.messages[0].content = text;
    }
    assert.deepEqual(result, copy);
    cases.push({
      name: `first-line-only-${array}-${newline.length}`,
      pass: true,
    });
  }
for (const content of [
  " " + original,
  "unknown",
  "other\n" + original,
  original + " suffix",
]) {
  const body = { messages: [{ role: "system", content }] };
  assert.equal(adapt(body), body);
  cases.push({ name: "unknown-unchanged-" + cases.length, pass: true });
}
fs.mkdirSync(process.env.FUSION_RELEASE_ROOT + "/evidence", { recursive: true });
fs.writeFileSync(
  process.env.FUSION_RELEASE_ROOT + "/evidence/first-line.json",
  JSON.stringify({ cases }, null, 2),
);
console.log(JSON.stringify({ cases }));
