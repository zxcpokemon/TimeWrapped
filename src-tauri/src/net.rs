//! Small HTTPS client using the Windows trust store. Only explicit service hosts.
use std::{ffi::c_void,ptr::{null,null_mut}};
use windows_sys::Win32::Networking::WinHttp::*;
type Result<T> = std::result::Result<T,String>;
fn wide(v:&str)->Vec<u16>{v.encode_utf16().chain(Some(0)).collect()}
struct Handle(*mut c_void);
impl Drop for Handle{fn drop(&mut self){unsafe{WinHttpCloseHandle(self.0);}}}
fn handle(v:*mut c_void)->Result<Handle>{if v.is_null(){Err("Не удалось открыть HTTPS-соединение Windows".into())}else{Ok(Handle(v))}}
pub fn get(host:&str,path:&str,key:Option<&str>)->Result<String>{
    if !["api.opendota.com","open.faceit.com","steamcommunity.com","www.faceit.com","musicbrainz.org"].contains(&host)||!path.starts_with('/')||path.contains(['\r','\n']){return Err("Недопустимый адрес сервиса".into());}
    if key.is_some_and(|k|k.contains(['\r','\n'])){return Err("Недопустимый API-ключ".into());}
    unsafe{
        let session=handle(WinHttpOpen(wide("TimeWrapped/0.5.0").as_ptr(),WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,null(),null(),0))?;
        WinHttpSetTimeouts(session.0,4000,6000,6000,10000);
        let connection=handle(WinHttpConnect(session.0,wide(host).as_ptr(),443,0))?;
        let request=handle(WinHttpOpenRequest(connection.0,wide("GET").as_ptr(),wide(path).as_ptr(),null(),null(),null(),WINHTTP_FLAG_SECURE))?;
        let policy=WINHTTP_OPTION_REDIRECT_POLICY_NEVER;
        if WinHttpSetOption(request.0,WINHTTP_OPTION_REDIRECT_POLICY,&policy as *const _ as _,4)==0{return Err("Не удалось ограничить перенаправления HTTPS".into());}
        let headers=wide(&format!("Accept: application/json, text/xml\r\n{}",key.map(|k|format!("Authorization: Bearer {k}\r\n")).unwrap_or_default()));
        if WinHttpSendRequest(request.0,headers.as_ptr(),(headers.len()-1)as u32,null(),0,0,0)==0||WinHttpReceiveResponse(request.0,null_mut())==0{return Err("Сервис недоступен: проверьте интернет или повторите позже".into());}
        let mut status=0u32;let mut size=4;
        if WinHttpQueryHeaders(request.0,WINHTTP_QUERY_STATUS_CODE|WINHTTP_QUERY_FLAG_NUMBER,null(),&mut status as *mut _ as _,&mut size,null_mut())==0{return Err("Не получен HTTP-статус".into());}
        if status!=200{return Err(match status{401|403 if host=="www.faceit.com"=>"FACEIT ограничил публичный доступ. Повторите позже или выберите режим Data API".into(),401|403 if host=="open.faceit.com"=>"FACEIT отклонил API-ключ. Проверьте ключ и доступ профиля".into(),401|403=>"Сервис отклонил запрос. Повторите позже".into(),404=>"Профиль не найден в этом сервисе".into(),429=>"Лимит запросов сервиса. Повторите позже".into(),_=>format!("Сервис вернул HTTP {status}")});}
        let mut output=Vec::new();let mut chunk=[0u8;16384];
        loop{let mut read=0;if WinHttpReadData(request.0,chunk.as_mut_ptr()as _,chunk.len()as u32,&mut read)==0{return Err("Ответ сервиса оборвался".into());}if read==0{break;}output.extend_from_slice(&chunk[..read as usize]);if output.len()>4_000_000{return Err("Ответ сервиса слишком большой".into());}}
        String::from_utf8(output).map_err(|_|"Некорректная кодировка ответа".into())
    }
}
pub fn json(host:&str,path:&str,key:Option<&str>)->Result<serde_json::Value>{serde_json::from_str(&get(host,path,key)?).map_err(|_|"Сервис вернул неожиданный формат данных".into())}
