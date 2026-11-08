import test from 'node:test';
import assert from 'node:assert/strict';
import {createExtensions} from '../src/extensions.mjs';
import {escapeHtml} from '../src/ui-model.mjs';
test('music screen emits balanced HTML with both totals inside its metrics container',()=>{
 const listeners={};globalThis.document={addEventListener:(name,fn)=>{listeners[name]=fn;}};
 const state={extensions:{config:{},music:{totals:[{service:'spotify',seconds:5},{service:'soundcloud',seconds:10}],recent:[]}}};
 const ui=createExtensions({state,invoke:async()=>{},esc:escapeHtml,duration:s=>String(s),render:()=>{},toast:()=>{},saveSetting:()=>{},insights:{renderGenreSettings:()=>''}});
 listeners.click({target:{closest:()=>({dataset:{extensionTab:'music'}})}});
 const html=ui.renderExtensions(),stack=[],voids=['input','img','br','hr','meta','link'];
 for(const match of html.matchAll(/<\/?([a-z][a-z0-9-]*)\b[^>]*>/gi)){
  const tag=match[1];if(voids.includes(tag))continue;
  if(match[0].startsWith('</'))assert.equal(stack.pop(),tag,`Unexpected closing tag ${match[0]}`);else stack.push(tag);
 }
 assert.deepEqual(stack,[]);
 assert.match(html,/<div class="extension-metrics">[\s\S]*Spotify · вся история[\s\S]*SoundCloud · вся история/);
});
