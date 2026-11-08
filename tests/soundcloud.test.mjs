import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import vm from 'node:vm';
const source=await readFile(new URL('../browser-extension/content.js',import.meta.url),'utf8');
function player({playing=true,hasControl=true,audio=[],position='0:30'}={}){
 const messages=[],timers=[],listeners={};
 const document={querySelector:selector=>selector==='.playbackSoundBadge__titleLink'?{getAttribute:()=>'<Song>'}:selector==='.playbackSoundBadge__lightLink'?{getAttribute:()=>'Artist'}:selector==='.playControl'?(hasControl?{classList:{contains:()=>playing}}:null):selector.includes('timePassed')?{textContent:position}:null,querySelectorAll:()=>audio,addEventListener:(event,fn)=>{listeners[event]=fn;}};
 const context=vm.createContext({document,chrome:{runtime:{id:'test',sendMessage:async m=>messages.push(m)}},setInterval:fn=>timers.push(fn),setTimeout:fn=>fn(),window:{addEventListener:()=>{}},MutationObserver:class{observe(){}}});
 vm.runInContext(source,context);return {messages,timers,listeners,context};
}
test('a paused unrelated audio element cannot hide the SoundCloud player state',()=>{
 const h=player({audio:[{paused:true,ended:false,muted:false,volume:1}]});assert.equal(h.messages[0].playing,true);assert.equal(h.messages[0].position,30);
});
test('paused player does not become playing because of another audio element',()=>{
 const h=player({playing:false,audio:[{paused:false,ended:false,muted:false,volume:1,currentTime:12}]});assert.equal(h.messages[0].playing,false);
});
test('the periodic worker sample recovers a reporter on an already-open tab without duplicate timers',()=>{
 const h=player();assert.equal(h.messages.length,1);vm.runInContext(source,h.context);assert.equal(h.messages.length,2);assert.equal(h.timers.length,1);
});
