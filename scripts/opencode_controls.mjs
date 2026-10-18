// SPDX-License-Identifier: Apache-2.0
// Official extension hook; never changes upstream identity or native tool schemas.
export default {
  id: "loadout.request-controls",
  async setup(context) {
    const base = process.env.LOADOUT_OPENCODE_CONTROL_URL;
    const authorization = "Basic " + Buffer.from("opencode:" + process.env.OPENCODE_SERVER_PASSWORD).toString("base64");
    await context.session.hook("http.request", async (event) => {
      if (event.kind !== "primary") return;
      const url = base + "/internal/control/" + encodeURIComponent(event.sessionID);
      const response = await fetch(url, { headers: { authorization }, signal: AbortSignal.timeout(5000) });
      if (!response.ok) throw new Error("Loadout request controls unavailable");
      const controls = await response.json();
      const body = await event.request.clone().json();
      if (controls.external_tools?.length) {
        const instruction = "This request is an external API tool handoff. The caller owns the workspace and executes tools. Only call these external MCP tools: " + controls.external_tools.join(", ") + ". Do not call this server's built-in tools. For an external execute tool, keep code referring to the caller's original tool names.";
        const lastSystem = body.messages.findLastIndex((message) => message.role === "system");
        body.messages.splice(lastSystem + 1, 0, { role: "system", content: instruction });
      }
      if (controls.max_tokens !== undefined) {
        delete body.max_completion_tokens;
        body.max_tokens = controls.max_tokens;
      }
      if (controls.tool_choice !== undefined) body.tool_choice = controls.tool_choice;
      event.request = new Request(event.request, { body: JSON.stringify(body) });
      const acknowledged = await fetch(url, {
        method: "POST", headers: { authorization, "Content-Type": "application/json" },
        body: JSON.stringify({ max_tokens: body.max_tokens, tool_choice: body.tool_choice, external_tools: controls.external_tools }),
        signal: AbortSignal.timeout(5000),
      });
      if (!acknowledged.ok) throw new Error("Loadout request control mismatch");
    }, { providerID: "opencode" });
    // Avoid an unrelated title-generation request racing the single-flight bridge.
    await context.session.hook("title", (event) => { event.result = "Loadout transient request"; });
    await context.session.hook("retry", (event) => { event.decision = { retry: false }; });
  },
};
