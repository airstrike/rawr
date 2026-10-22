use std::{collections::HashMap as StdMap, ffi::OsString, fmt, io::{self,BufRead}, os::unix::ffi::{OsStringExt,OsStrExt}, path::{Path,PathBuf}, str::FromStr, sync::OnceLock};
use serde::{de, de::IntoDeserializer, Deserialize};
use rustc_hash::FxHashMap as HashMap;
mod value;
use value::{Definition,Value,OptValue};
mod key { include!("../config-merge-key/reference.rs"); }
use key::ConfigKey;
impl fmt::Display for ConfigKey {
    fn fmt(&self,f:&mut fmt::Formatter<'_>)->fmt::Result { write!(f,"{}",self.parts().collect::<Vec<_>>().join(".")) }
}
#[path="../config-merge/program.rs"] mod program;
use program::ConfigValue;
use ConfigValue as CV;
#[derive(Debug)] struct ConfigError(String);
impl fmt::Display for ConfigError { fn fmt(&self,f:&mut fmt::Formatter<'_>)->fmt::Result { self.0.fmt(f) } }
impl std::error::Error for ConfigError {}
impl de::Error for ConfigError { fn custom<T:fmt::Display>(error:T)->Self { Self(error.to_string()) } }
impl ConfigError {
    fn new(error:String,_:Definition)->Self { Self(error) }
    fn expected(_: &ConfigKey,wanted:&str,_:&CV)->Self { Self(format!("expected {wanted}")) }
    fn missing(_: &ConfigKey)->Self { Self("missing".into()) }
    fn with_key_context(self,_:&ConfigKey,_:Option<Definition>)->Self { self }
}
type CargoResult<T> = Result<T,ConfigError>;
macro_rules! bail { ($($arg:tt)*) => { return Err(ConfigError(format!($($arg)*))) }; }
struct Env { env: StdMap<OsString,OsString> }
impl Env {
    fn get_str(&self,key:impl AsRef<std::ffi::OsStr>)->Option<&str> { self.env.get(key.as_ref()).and_then(|s|s.to_str()) }
    fn contains_key(&self,key:impl AsRef<std::ffi::OsStr>)->bool { self.env.contains_key(key.as_ref()) }
}
struct GlobalContext { ordinary:HashMap<String,CV>, credential_values:OnceLock<HashMap<String,CV>>, env:Env }
impl GlobalContext {
    fn values(&self)->CargoResult<&HashMap<String,CV>> { Ok(&self.ordinary) }
    fn env_keys(&self)->impl Iterator<Item=&str> { self.env.env.keys().filter_map(|key|key.to_str()) }
    fn check_environment_key_case_mismatch(&self,_:&ConfigKey) {}
}
include!("context.rs");

#[derive(Clone)] struct Deserializer<'gctx> { gctx:&'gctx GlobalContext,key:ConfigKey,env_prefix_ok:bool }
impl<'de> de::Deserializer<'de> for Deserializer<'_> {
    type Error=ConfigError;
    fn deserialize_any<V:de::Visitor<'de>>(self,_:V)->Result<V::Value,ConfigError> { Err(ConfigError("unreached method".into())) }
    fn deserialize_option<V:de::Visitor<'de>>(self,visitor:V)->Result<V::Value,ConfigError> {
        if self.gctx.has_key(&self.key,self.env_prefix_ok)? { visitor.visit_some(self) } else { visitor.visit_none() }
    }
    fn deserialize_struct<V:de::Visitor<'de>>(self,name:&'static str,fields:&'static [&'static str],visitor:V)->Result<V::Value,ConfigError> {
        assert_eq!(name,value::NAME); assert_eq!(fields,value::FIELDS);
        let source=ValueSource::with_deserializer(self)?;
        visitor.visit_map(ValueDeserializer::new(source))
    }
    fn deserialize_string<V:de::Visitor<'de>>(self,visitor:V)->Result<V::Value,ConfigError> {
        let v=self.gctx.get_string_priv(&self.key)?.ok_or_else(||ConfigError::missing(&self.key))?;
        let Value {val,definition}=v;
        let res:Result<V::Value,ConfigError>=visitor.visit_string(val);
        res.map_err(|error|error.with_key_context(&self.key,Some(definition)))
    }
    serde::forward_to_deserialize_any! { bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str bytes byte_buf unit unit_struct newtype_struct seq tuple tuple_struct map enum identifier ignored_any }
}
type ArrayItemKeyPath=();
struct ArrayItemDeserializer<'a> { cv:CV,key_path:&'a mut ArrayItemKeyPath }
impl<'de> de::Deserializer<'de> for ArrayItemDeserializer<'_> {
    type Error=ConfigError;
    fn deserialize_any<V:de::Visitor<'de>>(self,_:V)->Result<V::Value,ConfigError> { Err(ConfigError("unreached array protocol".into())) }
    serde::forward_to_deserialize_any! { bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string bytes byte_buf unit unit_struct newtype_struct seq tuple tuple_struct map struct option enum identifier ignored_any }
}
include!("protocol.rs");

fn definition(input:&serde_json::Value)->Definition {
    match input[0].as_str().unwrap() {
        "builtin"=>Definition::BuiltIn,
        "file"=>Definition::Path(input[1].as_str().unwrap().into()),
        "file-bytes"=>Definition::Path(OsString::from_vec(serde_json::from_value(input[1].clone()).unwrap()).into()),
        "env"=>Definition::Environment(input[1].as_str().unwrap().into()),
        "cli"=>Definition::Cli(input[1].as_str().map(Into::into)),
        _=>panic!("fixture definition"),
    }
}
fn origin(definition:&Definition)->serde_json::Value {
    match definition {
        Definition::BuiltIn=>serde_json::json!(["builtin",null]),
        Definition::Path(path)=>serde_json::json!(["file",path]),
        Definition::Environment(key)=>serde_json::json!(["env",key]),
        Definition::Cli(path)=>serde_json::json!(["cli",path]),
    }
}
fn table(input:&serde_json::Value)->HashMap<String,CV> {
    let CV::Table(table,_)=CV::parse(input["source"].as_str().unwrap(),definition(&input["definition"])).unwrap() else { unreachable!() };
    table
}
fn main() {
    std::panic::set_hook(Box::new(|_|{}));
    for line in io::stdin().lock().lines() {
        let mut row:serde_json::Value=serde_json::from_str(&line.unwrap()).unwrap();
        let mut key=ConfigKey::new();key.push("path-bases");key.push(row["base"].as_str().unwrap());
        let mut env=StdMap::new();
        if let Some(bytes)=row["environment"].as_array() { env.insert(key.as_env_key().into(),OsString::from_vec(bytes.iter().map(|v|v.as_u64().unwrap() as u8).collect())); }
        if row["prefix"].as_bool().unwrap() { env.insert(format!("{}_CHILD",key.as_env_key()).into(),"present".into()); }
        let gctx=GlobalContext { ordinary:table(&row["ordinary"]),credential_values:OnceLock::new(),env:Env{env} };
        if !row["credentials"].is_null() { assert!(gctx.credential_values.set(table(&row["credentials"])).is_ok()); }
        match Option::<Value<String>>::deserialize(Deserializer {gctx:&gctx,key,env_prefix_ok:true}) {
            Ok(None)=>row["result"]=serde_json::Value::Null,
            Ok(Some(value))=>{
                let path=std::panic::catch_unwind(||value.definition.root(Path::new("/work")).join(&value.val));
                row["result"]=serde_json::json!({"text":value.val,"origin":origin(&value.definition),"path":path.ok().map(|p|p.as_os_str().as_bytes().to_vec())});
            }
            Err(error)=>row["error"]=serde_json::json!(error.to_string()),
        }
        println!("{row}");
    }
}
