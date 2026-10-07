import assert from "node:assert/strict";
import { before, after, afterEach, test } from "node:test";
import { createServer } from "vite";
import fs from "node:fs";
let server, api, flashRootImage, requireResult;
before(async () => {
  server = await createServer({ server: { middlewareMode: true, watch: null, hmr: false, ws: false }, appType: "custom" });
  ({ api } = await server.ssrLoadModule("/src/lib/api/index.ts"));
  ({ flashRootImage, requireResult } = await server.ssrLoadModule("/src/lib/domain/rootTools.ts"));
});
afterEach(() => { delete globalThis.window; });
after(async () => { delete globalThis.window; await server.close(); });
test("every root tools facade command is registered in the native handler", () => {
  const source = fs.readFileSync("src/lib/api/rootTools.ts", "utf8");
  const backend = fs.readFileSync("src-tauri/src/lib.rs", "utf8").split("tauri::generate_handler![")[1];
  const commands = [...source.matchAll(/(?:transport\.result|rootWrite)[^\n]*?\("([a-z_]+)"/g)].map(m => m[1]);
  assert.equal(new Set(commands).size, 15);
  for (const command of commands) assert.match(backend, new RegExp(`::${command}\\s*[,]`));
});
test("root inspection has no browser success and preserves denied root without claiming stock", async () => {
  assert.equal((await api.rootInspect("phone")).ok, false);
  const state = { access: "denied", engine: "unknown", magiskMarkers: null, kernelsuMarkers: null };
  const calls=[];
  globalThis.window={__TAURI_INTERNALS__:{invoke:async(command,args)=>{calls.push({command,args});return state;}}};
  assert.deepEqual(await api.rootInspect("sha256:phone"),{ok:true,value:state});
  assert.deepEqual(calls,[{command:"root_inspect",args:{serial:"sha256:phone"}}]);
});
test("all new root tools writes remain blocked by default without contacting a device", async () => {
  let calls=0;globalThis.window={__TAURI_INTERNALS__:{invoke:async()=>{calls++;throw Error("unexpected");}}};
  for(const result of await Promise.all([
    api.rootModuleInstall("phone","a".repeat(64),true,true),
    api.rootModuleAction("phone","module","remove",true),
    api.rootSwitchPrepare("phone","C:/stock.img","resukisu",true),
    api.rootExternalPatchImport("phone","C:/stock.img","C:/patched.img",true),
    api.resukisuInstall("phone","a".repeat(64),true),
  ])) assert.equal(result.ok,false);
  assert.equal(calls,0);
});
test("package preparation pins selected ReSukiSU tag and never substitutes latest",async()=>{
  const calls=[];globalThis.window={__TAURI_INTERNALS__:{invoke:async(command,args)=>{calls.push({command,args});throw "certificate pin mismatch";}}};
  assert.deepEqual(await api.rootPackagePrepare("resukisu","v4.2.0-rc3"),{ok:false,error:"certificate pin mismatch"});
  assert.deepEqual(calls,[{command:"root_package_prepare",args:{id:"resukisu",tag:"v4.2.0-rc3"}}]);
});
test("manual image application requires unlocked fastbootd, never retries failed flash or reboots after failure",async()=>{
  const image={partition:"init_boot",path:"C:/한글 patched.img",sha256:"a".repeat(64),fingerprint:"Sony/current"};
  for(const vars of [null,{"is-userspace":"no",unlocked:"yes"},{"is-userspace":"yes",unlocked:"no"}]){
    await assert.rejects(()=>flashRootImage({fastbootGetvar:async()=>vars,fastbootFlash:async()=>assert.fail("flash called")},"phone",image));
  }
  const calls=[];
  const fake={fastbootGetvar:async()=>({"is-userspace":"yes",unlocked:"yes"}),fastbootFlash:async(...args)=>{calls.push(args);return{ok:false,error:"USB disconnected"};},fastbootReboot:async()=>assert.fail("reboot after failure")};
  await assert.rejects(()=>flashRootImage(fake,"phone",image),/USB disconnected/);assert.equal(calls.length,1);
  assert.deepEqual(calls[0],["init_boot",image.path,true,"phone",image.sha256]);
});
test("successful image flash is followed by one explicit OS reboot and native errors are retained",async()=>{
  const calls=[];const image={partition:"boot",path:"C:/boot.img",sha256:"a".repeat(64),fingerprint:"Sony/current"};
  await flashRootImage({fastbootGetvar:async()=>({"is-userspace":"yes",unlocked:"yes"}),fastbootFlash:async()=>{calls.push("flash");return{ok:true,value:null};},fastbootReboot:async(target,serial)=>{calls.push([target,serial]);return{ok:true,value:null};}},"phone",image);
  assert.deepEqual(calls,["flash",["os","phone"]]);assert.throws(()=>requireResult({ok:false,error:"not verified"}),/not verified/);
});
