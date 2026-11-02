// SPDX-License-Identifier: Apache-2.0
// Started by the supervisor just before it exits: brings the same instance back, and on request
// registers the WorkBuddy connector the new config describes. Nothing here prints secrets.
import { Instance, sleep } from "./runtime.mjs";

const [dir, connect] = process.argv.slice(2);
const previous = new Instance(dir);
for (let n = 0; n < 150 && (await previous.status()).status !== "stopped"; n++) await sleep(200);
const instance = new Instance(dir);
await instance.start();
if (connect === "workbuddy") {
  const wb = instance.meta.config.workbuddy;
  const url = `http://127.0.0.1:${wb.port}/v1`;
  const secrets = await instance.secrets();
  const { nodes } = await instance.admin("/api/provider-nodes");
  let node = nodes.find((n) => n.name === "workbuddy");
  if (node) await instance.admin(`/api/provider-nodes/${node.id}`, "PUT", { name: node.name, prefix: node.prefix, apiType: node.apiType, baseUrl: url });
  else
    node = (
      await instance.admin("/api/provider-nodes", "POST", {
        type: "openai-compatible",
        name: "workbuddy",
        prefix: "fusion-workbuddy",
        apiType: "chat",
        baseUrl: url,
      })
    ).node;
  const { connections } = await instance.admin("/api/providers");
  let account = connections.find((c) => c.provider === node.id);
  if (!account) {
    const created = await instance.admin("/api/providers", "POST", {
      provider: node.id,
      name: "main",
      apiKey: secrets.bridge,
      priority: 1,
      providerSpecificData: { baseUrl: url, apiType: "chat", prefix: node.prefix, nodeName: "workbuddy" },
    });
    account = created.connection;
    await instance.admin(`/api/providers/${account.id}`, "PUT", { isActive: true, testStatus: "active", healthCheckInterval: 0 });
  }
  await instance.admin(`/api/providers/${account.id}/sync-models`, "POST", {});
}
