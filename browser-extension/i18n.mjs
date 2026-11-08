import {messages} from './translations.mjs';
const languages=new Set(['ru','en','uk']);
let language='ru';
try{const saved=globalThis.localStorage?.getItem('timewrapped.language');if(languages.has(saved))language=saved;}catch{}
export const getLanguage=()=>language;
export const locale=()=>({ru:'ru-RU',en:'en-US',uk:'uk-UA'}[language]);
export function setLanguage(value){
 language=languages.has(value)?value:'ru';
 try{globalThis.localStorage?.setItem('timewrapped.language',language);}catch{}
 if(globalThis.document?.documentElement)document.documentElement.lang=language;
 return language;
}
export function t(value){
 const source=String(value??'');if(language==='ru')return source;
 const key=source.trim(),pair=messages[key];
 const translated=pair?.[language==='en'?0:1];
 return translated==null?source:source.replace(key,()=>translated);
}
export function localize(source){
 source=String(source);
 if(!/<[a-z!/]/i.test(source))return t(source);
 return source.split(/(<[^>]*>)/g).map(part=>part.startsWith('<')?part.replace(/\b(title|placeholder|aria-label|alt)=(['"])([\s\S]*?)\2/g,(_,name,quote,text)=>`${name}=${quote}${t(text)}${quote}`):t(part)).join('');
}
export function tr(strings,...values){
 const skeleton=strings.reduce((s,part,i)=>s+part+(i<values.length?`{${i}}`:''),'');
 // Translate source-owned text before inserting values: page titles, artists,
 // executable names and other user data never become translation candidates.
 return localize(skeleton).replace(/\{(\d+)\}/g,(_,i)=>String(values[Number(i)]??''));
}
export function bindStatic(root=globalThis.document){
 if(!root?.createTreeWalker)return ()=>{};
 const nodes=[],attributes=[];const walker=root.createTreeWalker(root.body||root,4);
 while(walker.nextNode()){
  const node=walker.currentNode;if(['SCRIPT','STYLE','TEXTAREA'].includes(node.parentElement?.tagName))continue;
  if(messages[node.nodeValue.trim()])nodes.push({node,source:node.nodeValue,last:node.nodeValue});
 }
 root.querySelectorAll('[title],[placeholder],[aria-label],[alt]').forEach(node=>{
  for(const name of ['title','placeholder','aria-label','alt']){const source=node.getAttribute(name);if(source&&messages[source.trim()])attributes.push({node,name,source});}
 });
 const apply=()=>{for(const item of nodes){if(item.node.nodeValue===item.last){item.last=t(item.source);item.node.nodeValue=item.last;}}for(const item of attributes)item.node.setAttribute(item.name,t(item.source));};
 apply();return apply;
}

export function visitCount(count){const n=Number(count)||0;const rule=new Intl.PluralRules(locale()).select(n);const words=language==='ru'?{one:'посещение',few:'посещения',many:'посещений',other:'посещения'}:language==='uk'?{one:'відвідування',few:'відвідування',many:'відвідувань',other:'відвідування'}:{one:'visit',other:'visits'};return `${n.toLocaleString(locale())} ${words[rule]||words.other}`;}
