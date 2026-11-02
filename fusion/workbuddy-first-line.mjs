// SPDX-License-Identifier: Apache-2.0
// Exact, opt-in WorkBuddy compatibility. No recursive or fuzzy replacement.
export const CODEX_FIRST_LINE =
  "You are a coding agent running in the Codex CLI, a terminal-based coding assistant. Codex CLI is an open source project led by OpenAI. You are expected to be precise, safe, and helpful.";
// Result of the pinned OmniRoute v3.8.52 WorkBuddy rules, verified in phase9.
export const WORKBUDDY_FIRST_LINE =
  "You are a coding agent running in the the assistant, a terminal-based coding assistant. The the assistant is an open source project led by the provider. You are expected to be precise, safe, and helpful.";
export function adaptWorkbuddyFirstLine(body) {
  const first = body?.messages?.[0];
  if (first?.role !== "system") return body;
  const parts = Array.isArray(first.content) ? first.content : null;
  if (parts && parts[0]?.type !== "text") return body;
  const text = parts ? parts[0].text : first.content;
  if (typeof text !== "string") return body;
  const end = text.indexOf("\n");
  const line = end < 0 ? text : text.slice(0, end);
  const cr = line.endsWith("\r") ? "\r" : "";
  if ((cr ? line.slice(0, -1) : line) !== CODEX_FIRST_LINE) return body;
  const changed = WORKBUDDY_FIRST_LINE + cr + (end < 0 ? "" : text.slice(end));
  const content = parts
    ? [{ ...parts[0], text: changed }, ...parts.slice(1)]
    : changed;
  return {
    ...body,
    messages: [{ ...first, content }, ...body.messages.slice(1)],
  };
}
