(()=>{
  if(globalThis.__timewrappedHistoryVersion===110){globalThis.__timewrappedHistoryReport?.();return;}
  globalThis.__timewrappedHistoryVersion=110;
  let previous=false,id=crypto.randomUUID(),lastLocation='',lastTitle='',navigationPending=true;
  function report(){
    const current=location.origin+location.pathname+location.search;
    if(lastLocation&&current!==lastLocation){id=crypto.randomUUID();navigationPending=true;}
    const metadataChanged=document.title!==lastTitle;
    lastLocation=current;lastTitle=document.title;
    const active=document.visibilityState==='visible'&&document.hasFocus();
    if(active||previous||navigationPending||metadataChanged){
      const visit=id;
      try{chrome.runtime.sendMessage({type:'site-visit',id,url:location.origin+location.pathname+location.search,title:document.title,active,navigation:navigationPending}).then(reply=>{if(id===visit&&(reply?.ok||reply?.paused))navigationPending=false;}).catch(()=>{});}catch{}
    }
    previous=active;
  }
  globalThis.__timewrappedHistoryReport=report;
  setInterval(report,5000);
  document.addEventListener('visibilitychange',report);
  for(const event of ['focus','blur','popstate','hashchange'])window.addEventListener(event,report);
  window.navigation?.addEventListener('currententrychange',report);
  const title=document.querySelector('title');if(title)new MutationObserver(report).observe(title,{childList:true,subtree:true,characterData:true});
  window.addEventListener('pagehide',()=>{try{chrome.runtime.sendMessage({type:'site-visit',id,url:location.origin+location.pathname+location.search,title:document.title,active:false,navigation:false}).catch(()=>{});}catch{}});
  report();
})();
