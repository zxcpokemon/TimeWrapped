import {t,tr,localize,locale,setLanguage,getLanguage,bindStatic} from '../src/i18n.mjs';
import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import vm from 'node:vm';
import {navigationPayload,withDeadline} from '../browser-extension/browser-model.mjs';
async function harness(values={},settings={}) {
 const listeners={},requests=[],injections=[],scripts=[],alarms=[],local={token:'a'.repeat(64),...values};
 const event=name=>({addListener:fn=>{listeners[name]=fn;}});
 const c={version:'1.0.1',tracking_enabled:true,sites_history:true,soundcloud:true,...settings.config};
 const chrome={runtime:{getURL:path=>'chrome-extension://test/'+path,onInstalled:event('installed'),onStartup:event('startup'),onMessage:event('message')},storage:{local:{get:async()=>({...local}),set:async v=>Object.assign(local,v)},onChanged:event('changed')},permissions:{contains:async()=>settings.permitted!==false,onRemoved:event('removed'),onAdded:event('added')},alarms:{create:async(name,info)=>alarms.push({name,...info}),onAlarm:event('alarm')},tabs:{onActivated:event('activated'),query:async query=>query.url?[{id:11,url:'https://soundcloud.com/artist/song'}]:[{id:10,url:'https://example.org'}]},scripting:{getRegisteredContentScripts:async()=>scripts,registerContentScripts:async a=>scripts.push(...a),unregisterContentScripts:async()=>scripts.splice(0),executeScript:async a=>{injections.push(a);if(settings.hangInjection)await new Promise(()=>{});}},windows:{get:async()=>({focused:true})}};
 const context=vm.createContext({t,tr,localize,locale,setLanguage,getLanguage,bindStatic,chrome,navigationPayload,withDeadline,URL,Date,AbortSignal,fetch:async(url,init)=>{requests.push({url,init});if(url.endsWith('browser-settings'))Object.assign(c,JSON.parse(init.body));return {ok:true,json:async()=>({...c})};}});
 const source=(await readFile(new URL('../browser-extension/background.js',import.meta.url),'utf8')).replace(/^import[^\n]+\n/gm,'');vm.runInContext(source,context);
 const send=(message,sender)=>new Promise(resolve=>{const pending=listeners.message(message,sender,resolve);if(!pending)resolve(undefined);});
 return {requests,local,send,injections,scripts,alarms,listeners,drain:async()=>{await listeners.alarm({name:'timewrapped-heartbeat'});await listeners.alarm({name:'timewrapped-heartbeat'});}};
}
const popup={url:'chrome-extension://test/popup.html'};
test('only companion pages can change application settings',async()=>{
 const h=await harness();assert.equal(await h.send({type:'companion-settings',settings:{adult_stats:true}},{url:'https://evil.example',tab:{id:1}}),undefined);assert.equal(h.requests.length,0);
 const c=await h.send({type:'companion-settings',settings:{soundcloud:true}},popup);assert.equal(c.version,'1.0.1');
 const writes=h.requests.filter(r=>r.init.method==='POST');assert.equal(writes.length,1);assert.equal(JSON.parse(writes[0].init.body).soundcloud,true);
});
test('browser pause stops site and SoundCloud reporting before any network request',async()=>{
 const h=await harness({paused:true});for(const type of ['soundcloud-state','site-visit'])assert.equal((await h.send({type,playing:true,active:true},{url:'https://soundcloud.com/test',tab:{id:1,active:true,windowId:2}})).paused,true);assert.equal(h.requests.length,0);
});
test('incognito and missing site access never submit browsing history',async()=>{
 const h=await harness({}, {permitted:false});await h.send({type:'site-visit'},{url:'https://example.org',tab:{id:1,active:true,windowId:2}});await h.send({type:'soundcloud-state',playing:true},{url:'https://soundcloud.com',tab:{id:1,incognito:true}});
 assert.equal(h.requests.filter(r=>r.url.endsWith('/visit')||r.url.endsWith('/soundcloud')).length,0);
});
test('native switch and granted access restore sites despite missing or stale legacy browser flag',async()=>{
 for(const values of [{},{historyEnabled:false}]){const h=await harness(values);const c=await h.send({type:'companion-config'},popup);assert.equal(c.browser_sites_ready,true);await h.drain();assert.equal(h.scripts[0].id,'timewrapped-history');assert.ok(h.injections.some(s=>s.target.tabId===10&&s.files[0]==='history.js'));}
});
test('native site opt-out unregisters a previously active reporter',async()=>{
 const h=await harness();await h.send({type:'companion-config'},popup);await h.drain();assert.equal(h.scripts.length,1);await h.send({type:'companion-settings',settings:{sites_history:false}},popup);await h.drain();assert.equal(h.scripts.length,0);
});
test('pairing and alarm both sample an already-open SoundCloud tab',async()=>{
 const h=await harness();await h.send({type:'companion-config'},popup);await h.drain();assert.ok(h.injections.some(s=>s.target.tabId===11&&s.files[0]==='content.js'));const before=h.injections.length;
 await h.listeners.alarm({name:'timewrapped-heartbeat'});assert.ok(h.injections.length>before);assert.equal(h.alarms[0].periodInMinutes,0.5);
});
test('site report reaches transport without a second local enable switch',async()=>{
 const h=await harness({historyEnabled:false});await h.send({type:'site-visit',id:'visit-1',active:true},{url:'https://example.org/path?secret=x',tab:{id:10,active:true,windowId:2}});
 const writes=h.requests.filter(r=>r.url.endsWith('/visit'));assert.equal(writes.length,1);assert.equal(JSON.parse(writes[0].init.body).url,'https://example.org');
});
test('one manifest and stable public key serve popup and settings',async()=>{
 const m=JSON.parse(await readFile(new URL('../browser-extension/manifest.json',import.meta.url),'utf8'));assert.equal(m.action.default_popup,'popup.html');assert.ok(m.key.length>200);assert.ok(m.permissions.includes('alarms'));assert.ok(m.permissions.includes('scripting'));assert.ok(!m.permissions.includes('history'));
});

test('popup connection must not wait for a frozen tab script injection',async()=>{
 const h=await harness({}, {hangInjection:true});
 const result=await Promise.race([h.send({type:'companion-config'},popup),new Promise(resolve=>setTimeout(()=>resolve('stuck'),100))]);
 assert.notEqual(result,'stuck');assert.equal(result.version,'1.0.1');
});
test('changing settings is acknowledged while script preparation is still blocked',async()=>{
 const h=await harness({}, {hangInjection:true});
 await h.send({type:'companion-config'},popup);
 const result=await Promise.race([h.send({type:'companion-settings',settings:{soundcloud:false}},popup),new Promise(resolve=>setTimeout(()=>resolve('stuck'),100))]);
 assert.notEqual(result,'stuck');assert.equal(result.soundcloud,false);
});
