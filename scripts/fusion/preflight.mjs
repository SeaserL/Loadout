// SPDX-License-Identifier: Apache-2.0
import fs from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";
import {root} from "./config.mjs";
const release=JSON.parse(fs.readFileSync(path.join(root,"runs/release-latest.json")));
const {preflight}=await import(pathToFileURL(path.join(release.extracted,"tools/preflight.mjs")));
const report=preflight(process.argv[2]);
fs.writeFileSync(path.join(root,"evidence/actual-preflight.json"),JSON.stringify({releaseId:release.releaseId,...report},null,2));
console.log(JSON.stringify({sourceCounts:report.sourceCounts,issues:report.issues,activationReady:report.activationReady}));
