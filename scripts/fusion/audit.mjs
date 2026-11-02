// SPDX-License-Identifier: Apache-2.0
import fs from "node:fs";
import path from "node:path";
import {pathToFileURL} from "node:url";
import {createHash} from "node:crypto";
import {spawnSync} from "node:child_process";
import {root, repo} from "./config.mjs";
const read=p=>JSON.parse(fs.readFileSync(p,"utf8"));
const release=read(path.join(root,"runs/release-latest.json"));
const {keytar,verifyPackage}=await import(pathToFileURL(path.join(release.extracted,"tools/runtime.mjs")));
verifyPackage();
const rows=[],directories=[],pidReuse=[];
const processRead=spawnSync("powershell.exe",["-NoProfile","-Command","Get-CimInstance Win32_Process | Select-Object ProcessId,ExecutablePath | ConvertTo-Json -Compress"],{encoding:"utf8",windowsHide:true});
if(processRead.status!==0)throw Error("PROCESS_AUDIT_FAILED");
const processes=new Map(JSON.parse(processRead.stdout).map(p=>[p.ProcessId,p.ExecutablePath]));
const ownedRoots=[release.extracted,read(path.join(root,"runs/old-baseline.json")).directory,

  path.join(root,"packages")];
function ownedAlive(pid){
  try{process.kill(pid,0);}catch{return false;}
  const exe=processes.get(pid);if(!exe)return true;
  const owned=ownedRoots.some(r=>exe.toLowerCase().startsWith(r.toLowerCase()+path.sep));
  if(!owned)pidReuse.push({pid,executable:exe});
  return owned;
}
async function walk(dir){for(const entry of fs.readdirSync(dir,{withFileTypes:true})){
 const p=path.join(dir,entry.name);if(entry.isSymbolicLink())throw Error("UNEXPECTED_LINK");
 if(entry.isDirectory())await walk(p);
 else if(entry.name==="instance.json"){
  const m=read(p);if(!/^loadout-test:omniroute-phase3-[a-f0-9-]+$/.test(m.service)||!/^loadout-test:fusion-bootstrap-[a-f0-9-]+$/.test(m.bootstrap))throw Error("UNEXPECTED_SCOPE");
  const entries=(await keytar.findCredentials(m.service)).length+(await keytar.findCredentials(m.bootstrap)).length;
  let active=false;const running=path.join(dir,"running.json");
  if(fs.existsSync(running)){const state=read(running);active=ownedAlive(state.pid)||ownedAlive(state.backendPid);}
  rows.push({directory:dir,entries,active});
 }
}}
for(const parent of [path.join(root,"runs/loadout-fusion-isolated"),path.join(root,"ui-final/runs/loadout-fusion-isolated")])
  if(fs.existsSync(parent)){directories.push(parent);await walk(parent);}
const manifest=read(path.join(release.extracted,"fusion-manifest.json"));
const sourceMismatch=manifest.files.filter(f=>f.path.startsWith("tools/")).filter(f=>{
 const source=path.join(repo,"fusion",f.path.slice(6));return !fs.existsSync(source)||createHash("sha256").update(fs.readFileSync(source)).digest("hex")!==f.sha256;
}).map(f=>f.path);
const result={releaseId:manifest.releaseId,instances:rows,directories,remaining:rows.reduce((n,r)=>n+r.entries,0),active:rows.filter(r=>r.active).length,sourceMismatch,pidReuse};
fs.writeFileSync(path.join(root,"evidence/final-audit.json"),JSON.stringify(result,null,2));
console.log(JSON.stringify({instances:rows.length,remaining:result.remaining,active:result.active,sourceMismatch}));
if(result.remaining||result.active||sourceMismatch.length)process.exitCode=1;
