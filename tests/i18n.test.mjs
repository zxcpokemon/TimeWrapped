import test from 'node:test';import assert from 'node:assert/strict';import {readFile} from 'node:fs/promises';
import {messages} from '../src/translations.mjs';import {t,tr,setLanguage,getLanguage,locale,visitCount} from '../src/i18n.mjs';
import {CATEGORY,duration,dateLabel} from '../src/ui-model.mjs';import {createExtensions} from '../src/extensions.mjs';import {createInsights} from '../src/insights.mjs';
test('English and Ukrainian catalogs are complete and keep every placeholder',()=>{
 for(const [key,pair]of Object.entries(messages)){assert.equal(pair.length,2);for(const value of pair){assert.ok(value.trim(),key);assert.deepEqual([...key.matchAll(/\{\d+\}/g)].map(m=>m[0]).sort(),[...value.matchAll(/\{\d+\}/g)].map(m=>m[0]).sort(),key);}assert.doesNotMatch(pair[0],/[А-Яа-яЁё]/,key);assert.doesNotMatch(pair[1],/[ыэёъЫЭЁЪ]/,key);}
});
test('language changes update units, categories and visit grammar without altering page titles',()=>{
 try{
  setLanguage('en');assert.equal(t('Настройки'),'Settings');assert.equal(locale(),'en-US');assert.equal(duration(3661),'1 h 1 min');assert.equal(CATEGORY.games.label,'Games');assert.equal(visitCount(1),'1 visit');
  const title='История';const html=tr`<h2>История посещений</h2><strong>${title}</strong>`;assert.equal(html,'<h2>Browsing history</h2><strong>История</strong>');
  setLanguage('uk');assert.equal(t('Настройки'),'Налаштування');assert.equal(duration(3661),'1 год 1 хв');assert.equal(CATEGORY.games.label,'Ігри');assert.equal(visitCount(5),'5 відвідувань');assert.match(dateLabel('2026-11-08'),/листопада/);
  setLanguage('ru');assert.equal(visitCount(1),'1 посещение');assert.equal(visitCount(2),'2 посещения');assert.equal(visitCount(5),'5 посещений');
 }finally{setLanguage('ru');}
});
test('extensions and browsing screens render fully in English and Ukrainian',()=>{
 const listeners={};globalThis.document={addEventListener:(name,fn)=>{listeners[name]=fn;},querySelector:()=>null};
 const state={page:'extensions',extensions:{config:{sites_history:true,site_details:true},music:{recent:[],totals:[]},bridge_status:'Готов к подключению браузерного дополнения'},extensionReport:{sites:[],recent:[],genres:[]}};
 const api={state,invoke:async()=>{},esc:s=>String(s??''),duration,render(){},toast(){},saveSetting(){},toggle(){return ''}};
 try{
  const insights=createInsights(api),extensions=createExtensions({...api,insights});
  for(const language of ['en','uk']){setLanguage(language);const browser=extensions.renderExtensions(),history=insights.renderWebsites();if(language==='en'){assert.doesNotMatch(browser,/[А-Яа-яЁё]/);assert.doesNotMatch(history,/[А-Яа-яЁё]/);}else{assert.match(browser,/Браузер/);assert.match(history,/Історія відвідувань/);}}
 }finally{setLanguage('ru');delete globalThis.document;}
});
test('browser and native catalogs match the desktop source',async()=>{
 for(const name of ['i18n.mjs','translations.mjs'])assert.deepEqual(await readFile(new URL('../src/'+name,import.meta.url)),await readFile(new URL('../browser-extension/'+name,import.meta.url)));
 assert.deepEqual(JSON.parse(await readFile(new URL('../src/translations.json',import.meta.url),'utf8')),messages);assert.equal(getLanguage(),'ru');
});
test('populated weekly and private reports render genre and site bars in every language',()=>{
 globalThis.document={addEventListener(){},querySelector(){return null;}};
 const state={page:'wrapped',extensions:{config:{adult_stats:true,music_genres:true}},extensionReport:{total_seconds:100,adult_seconds:20,music_seconds:50,identified_music_seconds:0,adult_visits:1,sites:[{site:'YouTube',domain:'youtube.com',seconds:80,visits:2},{site:'example.test',domain:'example.test',adult:true,seconds:20,visits:1}],genres:[{genre:'Не определён',seconds:50}],recent:[],days:[]}};
 const ui=createInsights({state,invoke:async()=>{},esc:s=>String(s??''),duration,render(){},toast(){}});
 try{for(const language of ['ru','en','uk']){setLanguage(language);assert.match(ui.renderSummary(),/insight-bar/);assert.match(ui.renderAdultPage(),/insight-bar/);}}finally{setLanguage('ru');delete globalThis.document;}
});
