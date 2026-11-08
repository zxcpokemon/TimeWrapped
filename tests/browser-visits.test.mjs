import test from 'node:test';import assert from 'node:assert/strict';import vm from 'node:vm';import {readFile} from 'node:fs/promises';
const source=await readFile(new URL('../browser-extension/history.js',import.meta.url),'utf8');
async function page(){
 const messages=[],events={},timers=[];let focus=true,sequence=0;
 const location={origin:'https://example.org',pathname:'/page',search:'?secret=not-sent'};
 const document={title:'Page one',visibilityState:'visible',hasFocus:()=>focus,querySelector:()=>null,addEventListener:(name,fn)=>{events[name]=fn;}};
 const context=vm.createContext({document,location,crypto:{randomUUID:()=>`page-${++sequence}`},setInterval:fn=>timers.push(fn),window:{addEventListener:(name,fn)=>{events[name]=fn;},navigation:{addEventListener:(name,fn)=>{events[name]=fn;}}},chrome:{runtime:{sendMessage:async message=>{messages.push(message);return {ok:true};}}}});
 vm.runInContext(source,context);await Promise.resolve();
 return {context,document,location,messages,events,timers,focus:value=>{focus=value;events[value?'focus':'blur']();}};
}
test('switching away and returning adds time samples without another page visit',async()=>{
 const p=await page();const id=p.messages[0].id;assert.equal(p.messages[0].navigation,true);
 p.focus(false);await Promise.resolve();p.focus(true);await Promise.resolve();
 assert.ok(p.messages.every(m=>m.id===id));assert.equal(p.messages.filter(m=>m.navigation).length,1);
 assert.equal(p.messages.at(-1).active,true);assert.ok(p.messages.every(m=>m.id===id));
});
test('real SPA navigation creates one new visit; a title update keeps its identity',async()=>{
 const p=await page();p.location.search='?video=next';p.events.currententrychange();await Promise.resolve();
 const id=p.messages.at(-1).id;assert.notEqual(id,p.messages[0].id);assert.equal(p.messages.at(-1).navigation,true);
 p.document.title='Final video title';p.timers[0]();await Promise.resolve();
 assert.equal(p.messages.at(-1).id,id);assert.equal(p.messages.at(-1).navigation,false);
 vm.runInContext(source,p.context);assert.equal(p.timers.length,1);
});
