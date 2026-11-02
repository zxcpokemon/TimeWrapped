export const CATEGORY = {
  games: {label:'Игры',color:'#a394ff'}, social:{label:'Общение',color:'#7ce4c3'},
  work:{label:'Работа',color:'#d9f784'}, study:{label:'Учёба',color:'#8dbbff'},
  entertainment:{label:'Развлечения',color:'#f1b886'}, creativity:{label:'Творчество',color:'#e99bc4'},
  other:{label:'Другое',color:'#89929c'}
};
export function localDate(date = new Date()) { return `${date.getFullYear()}-${String(date.getMonth()+1).padStart(2,'0')}-${String(date.getDate()).padStart(2,'0')}`; }
export function parseDate(value) { const [y,m,d]=value.split('-').map(Number); return new Date(y,m-1,d,12); }
export function addDays(value,days) { const d=parseDate(value);d.setDate(d.getDate()+days);return localDate(d); }
export function monday(value=localDate()) { const d=parseDate(value);d.setDate(d.getDate()-((d.getDay()+6)%7));return localDate(d); }
export function rangeFor(preset, today=localDate()) {
  if(preset==='yesterday') return {from:addDays(today,-1),to:addDays(today,-1)};
  if(preset==='7') return {from:addDays(today,-6),to:today};
  if(preset==='30') return {from:addDays(today,-29),to:today};
  if(preset==='week') return {from:monday(today),to:today};
  if(preset==='month') return {from:today.slice(0,8)+'01',to:today};
  return {from:today,to:today};
}
export function duration(seconds, compact=false) {
  const n=Math.max(0,Math.floor(Number(seconds)||0));const hours=Math.floor(n/3600),minutes=Math.floor(n%3600/60);
  if(hours) return `${hours} ч ${minutes} мин`;
  if(minutes) return `${minutes} мин`;
  return compact ? '0 мин' : `${n} сек`;
}
export function dateLabel(value, options={day:'numeric',month:'long'}) { return parseDate(value).toLocaleDateString('ru-RU',options); }
export function periodLabel(from,to) { return from===to?dateLabel(from,{day:'numeric',month:'long',year:'numeric'}):`${dateLabel(from,{day:'numeric',month:'short'})} — ${dateLabel(to,{day:'numeric',month:'short',year:'numeric'})}`; }
export function percentage(value,total) { return total>0?Math.max(0,Math.min(100,value/total*100)):0; }
export function escapeHtml(value) { return String(value??'').replace(/[&<>"']/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c])); }
export function initials(name) { return String(name||'?').replace(/\.exe$/i,'').split(/[\s._-]+/).map(s=>s[0]).filter(Boolean).slice(0,2).join('').toUpperCase(); }
export function emptyDashboard(from,to) { return {from,to,spotify_seconds:0,total_seconds:0,awake_seconds:0,sleep_seconds:0,active_seconds:0,locked_seconds:0,unattributed_seconds:0,app_total_seconds:0,focus_hours:[],idle_seconds:0,previous_seconds:0,change_percent:null,apps:[],categories:[],days:[],hours:[],switches:0}; }
export function changeText(value) { return value==null?'Нет данных для сравнения':`${value>0?'+':''}${Number(value).toLocaleString('ru-RU',{maximumFractionDigits:1})}% к предыдущему периоду`; }

export function appTotal(d) { return d.app_total_seconds ?? d.apps.reduce((sum,a)=>sum+a.seconds,0); }

export function visibleApps(apps) { return apps.filter(a=>a.seconds>0); }
export function backgroundApps(apps) { return apps.filter(a=>(a.seconds>0||a.running_seconds>0||a.background_seconds>0)&&!(/^(codex-|node_repl\.exe$|filewatcher\.exe$|defendersessionhelper\.exe$|vctip\.exe$|discordsystemhelper\.exe$)/i.test(a.exe||a.id))).slice().sort((a,b)=>(b.seconds||0)-(a.seconds||0)||(b.background_seconds||0)-(a.background_seconds||0)||(b.running_seconds||0)-(a.running_seconds||0)); }
