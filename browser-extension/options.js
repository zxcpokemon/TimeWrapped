import {t,tr,localize,locale,setLanguage,getLanguage,bindStatic} from './i18n.mjs';
import {withDeadline} from './browser-model.mjs';
const $=s=>document.querySelector(s), keys=['sites_history','soundcloud','spotify_titles','site_details','adult_stats'];
let paused=false,busy=false,browserLanguageOverride=false;
const translatePanel=bindStatic();setLanguage(getLanguage());
const message=async(type,extra={})=>{const r=await withDeadline(chrome.runtime.sendMessage({type,...extra}),6500,t('Дополнение не ответило. Нажмите «Подключить» ещё раз; если ошибка повторится, нажмите ↻ у TimeWrapped на странице расширений.'));if(!r||r.error)throw new Error(r?.error||t('Нет ответа дополнения'));return r;};
function error(e){$('#status').textContent=t(e.message||String(e));$('#connection').textContent=t('Нет подключения — откройте TimeWrapped и проверьте код');$('#connection').classList.remove('ok');}
function show(c){if(!browserLanguageOverride)setLanguage(c.language||'ru');translatePanel();$('#language').value=getLanguage();pauseLabel();keys.forEach(k=>{$('#'+k).checked=c[k]===true;$('#'+k).disabled=false;});$('#pause').disabled=false;$('#connection').classList.add('ok');$('#connection').textContent=c.tracking_enabled?t('Подключено · TimeWrapped ')+c.version:t('Подключено · сбор приостановлен в приложении');}
function pauseLabel(){$('#pause').textContent=paused?t('Возобновить сбор в браузере'):t('Приостановить сбор в браузере');}
async function connect(){if(busy)return;busy=true;$('#connect').disabled=true;$('#connection').textContent=t('Проверяем подключение…');try{const c=await message('companion-config');show(c);if(c.sites_history&&!c.browser_sites_ready){$('#sites_history').checked=false;$('#status').textContent=t('Включите историю сайтов здесь, чтобы разрешить доступ браузера.');}else $('#status').textContent=c.browser_paused?t('В браузере включена пауза. Нажмите «Возобновить сбор».'):(t(c.lastResult)||t('Настройки общие с приложением. Изменения сохраняются сразу.'));}catch(e){keys.forEach(k=>$('#'+k).disabled=true);$('#pause').disabled=true;error(e);}finally{busy=false;$('#connect').disabled=false;}}
withDeadline(chrome.storage.local.get(['token','paused','language']),2000,t('Браузер не ответил при чтении настроек')).then(async v=>{browserLanguageOverride=!!v.language;if(v.language)setLanguage(v.language);translatePanel();$('#language').value=getLanguage();$('#token').value=v.token||'';paused=!!v.paused;pauseLabel();if(v.token)await connect();else $('#connection').textContent=t('Вставьте код подключения из TimeWrapped');}).catch(error);
$('#pair').addEventListener('submit',async e=>{e.preventDefault();if(busy)return;try{await withDeadline(chrome.storage.local.set({token:$('#token').value.trim()}),2000,t('Не удалось сохранить код подключения'));await connect();}catch(e){error(e);$('#connect').disabled=false;}});
for(const key of keys)$('#'+key).addEventListener('change',async e=>{
 const enabled=e.target.checked;if(busy){e.target.checked=!enabled;return;}busy=true;
 try{
  if(key==='sites_history'&&enabled){const granted=await chrome.permissions.request({permissions:['tabs'],origins:['http://*/*','https://*/*']});if(!granted)throw new Error(t('Доступ не выдан. История сайтов остаётся выключенной.'));}
  const c=await message('companion-settings',{settings:{[key]:enabled}});
  if(key==='sites_history'){await message('history-permission-updated',{enabled});if(!enabled)await chrome.permissions.remove({permissions:['tabs'],origins:['http://*/*','https://*/*']}).catch(()=>{});}
  show(c);$('#status').textContent=enabled?t('Включено. Новая история начнётся сейчас.'):t('Выключено. Накопленная история сохранена.');
 }catch(e){$('#'+key).checked=!enabled;$('#status').textContent=t(e.message||String(e));}finally{busy=false;}
});
$('#pause').addEventListener('click',async()=>{paused=!paused;await chrome.storage.local.set({paused});pauseLabel();$('#status').textContent=paused?t('Сайты и SoundCloud приостановлены. Spotify управляется приложением.'):t('Сбор в браузере возобновлён.');});
$('#settings').addEventListener('click',()=>chrome.runtime.openOptionsPage());

$('#language').addEventListener('change',async event=>{browserLanguageOverride=true;setLanguage(event.target.value);await chrome.storage.local.set({language:getLanguage()});translatePanel();pauseLabel();$('#connection').textContent=t('Нажмите «Подключить», чтобы обновить состояние');});
