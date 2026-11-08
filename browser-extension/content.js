// One reporter per tab. Re-injection samples immediately without adding timers.
(()=>{
 if(globalThis.__timewrappedSoundCloud){globalThis.__timewrappedSoundCloud.report(true);return;}
 function seconds(text){const p=String(text||'').trim().split(':').map(Number);return p.length>=2&&p.every(Number.isFinite)?p.reduce((a,n)=>a*60+n,0):null;}
 function playerState(){
  const titleNode=document.querySelector('.playbackSoundBadge__titleLink');
  const artistNode=document.querySelector('.playbackSoundBadge__lightLink');
  const control=document.querySelector('.playControl');
  const audio=[...document.querySelectorAll('audio')].find(a=>!a.paused&&!a.ended&&!a.muted&&a.volume>0);
  const title=(titleNode?.getAttribute('title')||titleNode?.textContent||'').trim().slice(0,300);
  const artist=(artistNode?.getAttribute('title')||artistNode?.textContent||'').trim().slice(0,300);
  const positionNode=document.querySelector('.playbackTimeline__timePassed span[aria-hidden="true"]')||document.querySelector('.playbackTimeline__timePassed');
  const timeline=seconds(positionNode?.textContent);
  const position=timeline??(audio&&Number.isFinite(audio.currentTime)?audio.currentTime:null);
  // The page may contain unrelated paused audio elements. The player button is
  // authoritative when present; an arbitrary <audio> must not mask playback.
  const playing=!!title&&(control?control.classList.contains('playing'):!!audio);
  return {type:'soundcloud-state',title,artist,playing,position};
 }
 let previous='';
 function report(force=false){if(!chrome.runtime?.id)return;const state=playerState();const signature=JSON.stringify({...state,position:null});if(force||signature!==previous){previous=signature;try{chrome.runtime.sendMessage(state).catch(()=>{});}catch{}}}
 globalThis.__timewrappedSoundCloud={report};
 setInterval(()=>report(true),5000);
 document.addEventListener('click',()=>setTimeout(()=>report(),300),true);
 for(const event of ['play','pause','ended','seeking','seeked'])document.addEventListener(event,()=>report(true),true);
 const player=document.querySelector('.playControls');if(player)new MutationObserver(()=>report()).observe(player,{subtree:true,childList:true,attributes:true,attributeFilter:['class','title','aria-label']});
 window.addEventListener('pagehide',()=>{try{chrome.runtime.sendMessage({type:'soundcloud-state',title:'',artist:'',playing:false}).catch(()=>{});}catch{}});
 report(true);
})();
