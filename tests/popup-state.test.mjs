import {t,tr,localize,locale,setLanguage,getLanguage,bindStatic} from '../src/i18n.mjs';
import test from 'node:test';
import assert from 'node:assert/strict';
import vm from 'node:vm';
import {readFile} from 'node:fs/promises';
import {withDeadline} from '../browser-extension/browser-model.mjs';
const source=(await readFile(new URL('../browser-extension/options.js',import.meta.url),'utf8')).replace(/^import[^\n]+\n/gm,'');
function popup(runtimeReply,storageFailure=false){
 const elements=new Map();const element=id=>{if(!elements.has(id))elements.set(id,{value:'',textContent:'',disabled:false,checked:false,listeners:{},classList:{add(){},remove(){}},addEventListener(event,fn){this.listeners[event]=fn;}});return elements.get(id);};
 const chrome={storage:{local:{get:async()=>{if(storageFailure)throw Error('Storage unavailable');return {token:'a'.repeat(64)};},set:async()=>{}}},runtime:{sendMessage:runtimeReply,openOptionsPage(){}},permissions:{contains:async()=>true}};
 const context=vm.createContext({t,tr,localize,locale,setLanguage,getLanguage,bindStatic,document:{querySelector:element},chrome,withDeadline:(task,ms,message)=>withDeadline(task,Math.min(ms,20),message)});
 vm.runInContext(source,context);return {element};
}
test('a silent worker leaves the loading state and allows retry',async()=>{
 let reply=()=>new Promise(()=>{});const p=popup(()=>reply());
 await new Promise(resolve=>setTimeout(resolve,60));
 assert.match(p.element('#connection').textContent,/Нет подключения/);
 assert.match(p.element('#status').textContent,/Дополнение не ответило/);
 assert.equal(p.element('#connect').disabled,false);
 reply=async()=>({version:'1.0.2',tracking_enabled:true,sites_history:true,browser_sites_ready:true,soundcloud:true});
 await p.element('#pair').listeners.submit({preventDefault(){}});
 assert.match(p.element('#connection').textContent,/Подключено/);
 assert.equal(p.element('#soundcloud').disabled,false);assert.equal(p.element('#soundcloud').checked,true);
});
test('failed local storage shows an error instead of leaving the startup spinner',async()=>{
 const p=popup(async()=>({}),true);await new Promise(resolve=>setTimeout(resolve,30));
 assert.match(p.element('#connection').textContent,/Нет подключения/);
 assert.match(p.element('#status').textContent,/Storage unavailable/);
});
