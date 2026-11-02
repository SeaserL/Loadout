// SPDX-License-Identifier: Apache-2.0
import fs from "node:fs";
import path from "node:path";
import { spawn } from "node:child_process";
import { root, repo, source } from "./config.mjs";
const read = p => JSON.parse(fs.readFileSync(p, "utf8"));
const release = read(path.join(root, "runs/release-latest.json"));
const groups = [
  ["core",43], ["pacing",23], ["management",8], ["account-rules",12],
  ["migration",7], ["workbuddy-chain",4], ["first-line",10,"first-line.json"], ["connector",13,"offline.json"],
  ["readonly",20,"readonly-latest.json"], ["radar",23,"radar-latest.json"],
  ["pricing",25,"pricing-latest.json"], ["subscription",18,"subscription-latest.json"],
  ["quota-model",22,"quota-model-latest.json"], ["upgrade",4,"upgrade-result.json"], ["preflight",8,"preflight-test.json"],
  ["console",7,"console.json"],
  ["desktop",7,"desktop.json"],
  ["control",21,"control.json"],
];
const rows = [];
for (const [name, expected, resultName] of groups) {
  const logPath = path.join(root,"evidence",name+".log"), fd = fs.openSync(logPath,"w");
  console.log("RUN " + name);
  const child = spawn(path.join(release.extracted,"node.exe"),[path.join(repo,"tests/fusion",name+"-test.mjs")],{
    cwd:repo, env:{...process.env,FUSION_PACKAGE:release.extracted,FUSION_UPSTREAM:source},
    stdio:["ignore","pipe","pipe"],windowsHide:true,
  });
  for(const stream of [child.stdout,child.stderr]) stream.on("data",data=>fs.writeSync(fd,data));
  const code = await new Promise((resolve,reject)=>{child.on("error",reject);child.on("exit",resolve);});
  fs.closeSync(fd);
  const text=fs.readFileSync(logPath,"utf8"), matches=[...text.matchAll(/^RESULT (.+)$/gm)];
  const resultPath=resultName?path.join(root,"evidence",resultName):matches.length===1?matches[0][1].trim():null;
  const result=resultPath&&fs.existsSync(resultPath)?read(resultPath):{fatal:"RESULT_MISSING",cases:[]};
  fs.writeFileSync(path.join(root,"evidence",name+"-result.json"),JSON.stringify(result,null,2));
  const row={name,code,expected,total:result.cases?.length??0,passed:result.cases?.filter(c=>c.pass).length??0,
    fatal:result.fatal??result.failure??null,...(result.benchmarks?{benchmarks:result.benchmarks}:{})};
  row.ok=code===0&&!row.fatal&&row.total===expected&&row.passed===expected;
  rows.push(row);console.log(JSON.stringify(row));
  fs.writeFileSync(path.join(root,"evidence/regression.json"),JSON.stringify({releaseId:release.releaseId,groups:rows,
    expectedGroups:groups.length,passed:rows.reduce((n,r)=>n+r.passed,0),complete:rows.length===groups.length&&rows.every(r=>r.ok)},null,2));
  if(!row.ok){process.exitCode=1;break;}
}
