// SPDX-License-Identifier: Apache-2.0
import fs from "node:fs";
import http from "node:http";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { Instance, freePort, isolatedPath, packageRoot, verifyPackage } from "./runtime.mjs";

// Portable control-plane entry. Backend ownership remains with the existing supervisor.
const USUAL_PORT = 8318;
export function desktopDirectory(argument) {
  return isolatedPath(argument ?? path.join(path.dirname(packageRoot), "loadout-fusion-isolated", "desktop-current"));
}
/** The port if nothing holds it, otherwise any free one. */
export async function choosePort(wanted) {
  const probe = http.createServer();
  const free = await new Promise((resolve) => {
    probe.once("error", () => resolve(false));
    probe.listen(wanted, "127.0.0.1", () => resolve(true));
  });
  if (!free) return freePort();
  await new Promise((resolve) => probe.close(resolve));
  return wanted;
}
export async function prepareDesktop(argument) {
  verifyPackage(false);
  const dir = desktopDirectory(argument);
  const instance = fs.existsSync(path.join(dir, "instance.json"))
    ? new Instance(dir)
    // Clients are configured with this address, so it is chosen once and stays. Another program
    // on the usual port moves a new install for good, not from run to run.
    : await Instance.create(dir, undefined, { network: "direct", port: argument ? await freePort() : await choosePort(USUAL_PORT) });
  let state = await instance.status();
  if (state.status !== "ready") state = await instance.start();
  if (state.status !== "ready" || state.port === 8317 || state.port <= 1024)
    throw Error("ISOLATED_BACKEND_NOT_READY");
  return { instance, target: { instance: dir, url: `http://127.0.0.1:${state.port}/loadout/index.html` } };
}
async function main() {
  const [command = "prepare", argument, ...extra] = process.argv.slice(2);
  if (extra.length || !["prepare", "prepare-session", "stop"].includes(command)) throw Error("DESKTOP_ARGUMENTS");
  if (command === "stop") {
    await new Instance(desktopDirectory(argument)).stop();
    return { status: "stopped" };
  }
  const { instance, target } = await prepareDesktop(argument);
  // The shell reads this over a private pipe; it never goes to a file or a command line.
  if (command === "prepare-session") return { url: target.url, session: await instance.desktopSession() };
  return target;
}
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().then(value => console.log(JSON.stringify(value))).catch((error) => {
    // Do not echo backend errors or credentials; these categories are defined by this package.
    const known = /^[A-Z][A-Z_0-9]{2,60}$/.test(error?.message ?? "") ? error.message : "DESKTOP_START_OR_STOP_FAILED";
    console.error(known);
    process.exitCode = 1;
  });
}
