// SPDX-License-Identifier: Apache-2.0
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { DatabaseSync } from "node:sqlite";
import { spawnSync } from "node:child_process";
import { load, fingerprint, runs, sha, pkg } from "./harness.mjs";
const { assess, preflight } = await load("preflight.mjs");
const cases=[];
const check=(name,fn)=>{fn();cases.push({name,pass:true});};
const state={version:1,accounts:[{ref:"p/account-1",provider:"p",kind:"deepseek",enabled:true}],
  pools:[{name:"old",ordering:"manual",members:[{account:"p/account-1",model:"m",enabled:true}]}],
  endpoints:[],accountSetCount:0,clientPolicies:[{allowed_models:["loadout/old"]}],accountModels:[{account:"p/account-1",model:"m"}]};
const proposal={bindings:{"p/account-1":{provider:"native-p",connectionId:"synthetic",models:["m"]}},
  aliases:Object.fromEntries(["default","fast","realtime"].map(n=>["loadout/"+n,[{account:"p/account-1",model:"m"}]]))};
check("required-account-unbound-is-not-ready",()=>{const r=assess(state);assert(!r.activationReady);assert(r.issues.includes("REQUIRED_ACCOUNT_BINDINGS_INCOMPLETE"));assert.equal(r.pools[0].members.length,1);});
check("defaults-are-not-invented-from-test-model",()=>{const r=assess(state);assert.equal(r.aliases.find(a=>a.alias==="loadout/default").status,"EXPLICIT_MAPPING_REQUIRED");});
check("complete-proposal-is-not-live-credential-proof",()=>{const r=assess(state,proposal);assert(r.mapping.structurallyReady);assert(!r.activationReady);assert(r.accounts.every(a=>a.status==="PROPOSED_NOT_VERIFIED"));});
check("unknown-client-alias-blocked",()=>{const s=structuredClone(state);s.clientPolicies[0].allowed_models=["missing/alias"];assert(assess(s,proposal).issues.includes("CLIENT_ALIAS_MAPPING_REQUIRED"));});
check("disabled-pool-member-is-not-dropped",()=>{const s=structuredClone(state);s.pools[0].members[0].enabled=false;const r=assess(s,proposal);assert(!r.mapping.structurallyReady);assert.equal(r.mapping.routes[0].missing[0].reason,"DISABLED_MEMBER");});
check("stale-proposal-rejected",()=>{assert(assess(state,{...proposal,sourceFingerprint:fingerprint({...state,version:0})}).issues.includes("STALE_SOURCE"));});
check("foreign-binding-rejected",()=>{assert(assess(state,{...proposal,bindings:{...proposal.bindings,foreign:{}}}).issues.includes("UNKNOWN_OR_DISABLED_BINDING"));});
check("sqlite-read-only-and-sensitive-columns-not-exported",()=>{
  const dir=path.join(runs,"preflight-"+Date.now());fs.mkdirSync(dir);const file=path.join(dir,"source.db");
  const db=new DatabaseSync(file);
  db.exec(`CREATE TABLE providers(id TEXT,slug TEXT,kind TEXT);
CREATE TABLE accounts(id TEXT,provider_id TEXT,enabled INTEGER,position INTEGER,secret TEXT);
CREATE TABLE pools(id TEXT,name TEXT,ordering TEXT);
CREATE TABLE account_models(id TEXT,account_id TEXT,upstream_model_id TEXT,enabled INTEGER);
CREATE TABLE pool_members(pool_id TEXT,account_model_id TEXT,position INTEGER);
CREATE TABLE endpoints(name TEXT,provider_slug TEXT,members_json TEXT,wait_secs INTEGER,rotate INTEGER,default_model TEXT,enabled INTEGER);
CREATE TABLE clients(policy_json TEXT,enabled INTEGER,revoked_at TEXT,secret TEXT);
CREATE TABLE account_sets(id TEXT);
INSERT INTO providers VALUES('p','synthetic','deepseek');
INSERT INTO accounts VALUES('private-id','p',1,0,'synthetic-secret-must-not-export');
INSERT INTO clients VALUES('{}',1,NULL,'synthetic-secret-must-not-export');`);
  db.close();const before=sha(fs.readFileSync(file)), report=preflight(file);
  assert.equal(before,sha(fs.readFileSync(file)));assert(report.metadataStableDuringRead);
  assert(!JSON.stringify(report).includes("synthetic-secret-must-not-export"));assert(!JSON.stringify(report).includes("private-id"));
  const cli=spawnSync(path.join(pkg,"node.exe"),[path.join(pkg,"tools/cli.mjs"),"preflight",file],{encoding:"utf8",windowsHide:true});
  assert.equal(cli.status,0);assert.equal(JSON.parse(cli.stdout).activationReady,false);assert.equal(before,sha(fs.readFileSync(file)));
});
fs.writeFileSync(path.join(process.env.FUSION_RELEASE_ROOT,"evidence/preflight-test.json"),JSON.stringify({cases},null,2));
