// SPDX-License-Identifier: Apache-2.0
// Explicit client profile for Loadout's single-flight OpenCode bridge.
export default {
  id: "loadout.client-profile",
  async setup(context) {
    await context.session.hook("title", (event) => {
      if (event.model.providerID === "loadout") event.result = "Loadout coding session";
    });
    if (context.options.noRetry === true) {
      await context.session.hook("retry", (event) => {
        if (event.model.providerID === "loadout") event.decision = { retry: false };
      });
    }
  },
};
