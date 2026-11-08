use serde_json::Value;use std::sync::OnceLock;
pub fn text(language:&str,source:&str)->String {
 if language=="ru"{return source.into();}
 static CATALOG:OnceLock<Value>=OnceLock::new();let catalog=CATALOG.get_or_init(||serde_json::from_str(include_str!("../../src/translations.json")).unwrap_or(Value::Null));
 catalog.get(source).and_then(|v|v.get(if language=="uk"{1}else{0})).and_then(Value::as_str).unwrap_or(source).to_owned()
}
