import {t,tr,localize,locale,setLanguage} from './i18n.mjs';
import {navigationPayload,withDeadline} from './browser-model.mjs';
const SOUND=['https://soundcloud.com/*','https://www.soundcloud.com/*'];
const SITE_ACCESS={permissions:['tabs'],origins:['https://*/*','http://*/*']};
let activeMusicTab=null,configuration=null,configurationTime=0,lastStatus='',lastLog=0,syncPromise=null,pendingSync=null,browserLanguage=null;
async function api(path,body){
 const {token}=await withDeadline(chrome.storage.local.get('token'),2000,t('Не удалось прочитать код подключения'));
 if(!/^[a-f0-9]{64}$/.test(token||''))throw new Error(t('Вставьте код подключения в настройках дополнения'));
 const response=await withDeadline(fetch('http://127.0.0.1:38671/'+path,{method:body?'POST':'GET',headers:{Authorization:`Bearer ${token}`,...(body?{'Content-Type':'application/json'}:{})},...(body?{body:JSON.stringify(body)}:{}),signal:AbortSignal.timeout(4000)}),4500,t('TimeWrapped не ответил. Проверьте, что приложение запущено.'));
 if(!response.ok)throw new Error(response.status===401?t('Неверный код подключения. Скопируйте его заново из приложения.'):t('Откройте TimeWrapped и проверьте подключение'));
 return withDeadline(response.json(),1000,t('Не удалось получить ответ TimeWrapped'));
}
async function config(force=false){if(force||!configuration||Date.now()-configurationTime>15000){configuration=await api('browser-config');configurationTime=Date.now();setLanguage(browserLanguage||configuration.language||'ru');}return configuration;}
async function status(text){if(text!==lastStatus||Date.now()-lastLog>30000){lastStatus=text;lastLog=Date.now();await chrome.storage.local.set({lastResult:text,lastSeen:Date.now()});}}
async function historyAccess(){return withDeadline(chrome.permissions.contains(SITE_ACCESS),2000,t('Браузер не ответил на проверку разрешений'));}
const browserCall=promise=>withDeadline(promise,2500,t('Вкладка браузера не ответила. Обновите её страницу.'));
async function registerHistory(c,inject=false){
 const {paused}=await chrome.storage.local.get('paused');
 const enabled=c.sites_history&&!paused&&await historyAccess();
 const scripts=await browserCall(chrome.scripting.getRegisteredContentScripts());
 if(!enabled){if(scripts.some(s=>s.id==='timewrapped-history'))await browserCall(chrome.scripting.unregisterContentScripts({ids:['timewrapped-history']}));return false;}
 const fresh=!scripts.some(s=>s.id==='timewrapped-history');
 if(fresh)await browserCall(chrome.scripting.registerContentScripts([{id:'timewrapped-history',matches:['http://*/*','https://*/*'],js:['history.js'],runAt:'document_idle',persistAcrossSessions:true}]));
 if(inject||fresh){const tabs=await browserCall(chrome.tabs.query({active:true}));await Promise.allSettled(tabs.filter(tab=>!tab.incognito&&/^https?:/.test(tab.url||'')).map(tab=>browserCall(chrome.scripting.executeScript({target:{tabId:tab.id},files:['history.js'],injectImmediately:true}))));}
 return true;
}
async function sampleSoundCloud(c){
 const {paused}=await chrome.storage.local.get('paused');if(paused||!c.soundcloud)return;
 const tabs=await browserCall(chrome.tabs.query({url:SOUND}));
 await Promise.allSettled(tabs.filter(tab=>!tab.incognito&&!tab.discarded).map(tab=>browserCall(chrome.scripting.executeScript({target:{tabId:tab.id},files:['content.js'],injectImmediately:true}))));
}
async function synchronize(inject=false,knownConfig=null){
 if(syncPromise){if(knownConfig)pendingSync={inject,knownConfig};return syncPromise;}
 const task=(async()=>{const c=knownConfig||await config(true);
  // One frozen tab cannot block another service or the popup's connection.
  const results=await Promise.allSettled([registerHistory(c,inject),sampleSoundCloud(c)]);
  const failed=results.find(r=>r.status==='rejected');if(failed)await status(failed.reason?.message||String(failed.reason));
 })();
 syncPromise=withDeadline(task,10000,t('Подготовка вкладок превысила время ожидания'));
 try{return await syncPromise;}finally{syncPromise=null;if(pendingSync){const next=pendingSync;pendingSync=null;void synchronize(next.inject,next.knownConfig).catch(e=>status(e.message||String(e)));}}
}
async function companionState(c){
 const [permitted,local,saved]=await Promise.all([historyAccess(),withDeadline(chrome.storage.local.get('paused'),2000),withDeadline(chrome.storage.local.get(['lastResult','lastSeen']),2000)]);
 return {...c,browser_sites_ready:!!(c.sites_history&&permitted&&!local.paused),browser_paused:!!local.paused,lastResult:saved.lastResult,lastSeen:saved.lastSeen};
}

withDeadline(chrome.storage.local.get('language'),2000).then(v=>{browserLanguage=v.language||null;if(browserLanguage)setLanguage(browserLanguage);}).catch(()=>{});
const resume=()=>synchronize(true).catch(e=>status(e.message||String(e)));
chrome.runtime.onInstalled.addListener(resume);
chrome.runtime.onStartup.addListener(resume);
Promise.resolve(chrome.alarms.create('timewrapped-heartbeat',{periodInMinutes:0.5})).catch(()=>{});
chrome.alarms.onAlarm.addListener(alarm=>{if(alarm.name==='timewrapped-heartbeat')return synchronize().catch(e=>status(e.message||String(e)));});
chrome.permissions.onRemoved.addListener(resume);
chrome.permissions.onAdded.addListener(resume);
chrome.tabs.onActivated.addListener(resume);
chrome.storage.onChanged.addListener((changes,area)=>{if(area==='local'&&changes.language){browserLanguage=changes.language.newValue||null;setLanguage(browserLanguage||configuration?.language||'ru');}if(area==='local'&&(changes.token||changes.paused)){configuration=null;resume();}});
chrome.runtime.onMessage.addListener((message,sender,reply)=>{
 const internal=[chrome.runtime.getURL('popup.html'),chrome.runtime.getURL('options.html')].includes(sender.url);
 if(['companion-config','companion-settings','history-permission-updated'].includes(message?.type)){
  if(!internal)return;
  (async()=>{
   let c;if(message.type==='companion-settings'){c=await api('browser-settings',message.settings);configuration=c;configurationTime=Date.now();}else{c=await config(true);}
   const state=await companionState(c);
   // Reply without awaiting script injection, tab loading, or another sync job.
   void synchronize(true,c).catch(e=>status(e.message||String(e)));
   return state;
  })().then(reply).catch(e=>reply({error:String(e.message||e)}));return true;
 }
 if(!sender.url||sender.tab?.incognito||!['soundcloud-state','site-visit'].includes(message?.type))return;
 (async()=>{
  const {paused}=await chrome.storage.local.get('paused');if(paused)return {paused:true};
  const c=await config();
  if(message.type==='soundcloud-state'){
   if(!['soundcloud.com','www.soundcloud.com'].includes(new URL(sender.url).hostname))return;
   if(!c.soundcloud)return {paused:true};
   if(message.playing===true)activeMusicTab=sender.tab?.id;else if(activeMusicTab===null||activeMusicTab!==sender.tab?.id)return;
   const result=await api('soundcloud',{title:String(message.title||'').slice(0,300),artist:String(message.artist||'').slice(0,300),playing:message.playing===true,position:Number.isFinite(message.position)?message.position:null});
   await status(result.paused?t('TimeWrapped: запись музыки приостановлена'):message.playing?t('SoundCloud: передаём воспроизведение'):t('SoundCloud: пауза'));return result;
  }
  if(!c.sites_history||!await historyAccess())return {paused:true};
  const window=await chrome.windows.get(sender.tab.windowId);
  const payload=navigationPayload({...message,url:sender.url},{details:c.site_details===true,incognito:sender.tab.incognito,active:sender.tab.active&&window.focused});
  if(!payload)return;const result=await api('visit',payload);await status(t('Подключено: история активных сайтов'));return result;
 })().then(reply).catch(async e=>{await status(String(e.message||e));reply({error:String(e.message||e)});});return true;
});
