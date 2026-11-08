export function safePagePath(url){
 let path=url.pathname;const video=url.searchParams.get('v');
 if((url.hostname==='youtube.com'||url.hostname.endsWith('.youtube.com'))&&path==='/watch'&&/^[A-Za-z0-9_-]{11}$/.test(video||''))path+='?v='+video;
 return path;
}
export function navigationPayload(message,{details=false,incognito=false,active=false}={}){
  if(incognito)return null;
  let url;try{url=new URL(message.url);}catch{return null;}
  if(!['http:','https:'].includes(url.protocol)||url.username||url.password||!url.hostname.includes('.'))return null;
  return {id:String(message.id||'').slice(0,100),url:details?url.origin+safePagePath(url):url.origin,title:details?String(message.title||'').slice(0,250):'',active:active&&message.active===true,...(typeof message.navigation==='boolean'?{navigation:message.navigation}: {})};
}

export function withDeadline(task,milliseconds,message='Истекло время ожидания'){
  return new Promise((resolve,reject)=>{
    const timer=setTimeout(()=>reject(new Error(message)),milliseconds);
    Promise.resolve(task).then(value=>{clearTimeout(timer);resolve(value);},error=>{clearTimeout(timer);reject(error);});
  });
}
