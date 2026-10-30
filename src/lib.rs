#![recursion_limit = "512"]
//! Crate map. Each group is one stage of turning a site response into a
//! page: `app` serves HTTP, `network` talks to the site, `protocol` runs the
//! challenge flow, `syntax` reads the initializer without executing it,
//! `program` decodes the protocol bytecode, `tasks` computes the finite task
//! models, `sites` holds per-site adapters and book sources.
pub mod app;
mod cpu;
pub mod network;
pub mod program;
pub mod protocol;
#[cfg(test)]
mod regression;
pub mod sites;
pub mod syntax;
pub mod tasks;
pub mod version;
use serde_json::{json, Value};
use std::{
    fmt,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Debug, Clone, PartialEq)]
pub struct Error {
    pub code: String,
    pub message: String,
    pub status: u16,
    pub details: Value,
}
pub type Result<T> = std::result::Result<T, Error>;
impl Error {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            status: 422,
            details: Value::Null,
        }
    }
    pub fn reply(&self) -> Value {
        json!({"ok":false,"code":self.code,"error":self.message,"details":self.details})
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for Error {}
impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Self::new("INVALID_DATA", e.to_string())
    }
}
impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self::new("IO_ERROR", e.to_string())
    }
}
impl From<base64::DecodeError> for Error {
    fn from(e: base64::DecodeError) -> Self {
        Self::new("INVALID_DATA", e.to_string())
    }
}
pub fn require(condition: bool, message: &str) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(Error::new("UNSUPPORTED_FORMAT", message))
    }
}
pub fn epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
pub fn string(v: &Value) -> Result<&str> {
    v.as_str()
        .ok_or_else(|| Error::new("INVALID_DATA", "Expected string"))
}
pub fn uint(v: &Value) -> Result<usize> {
    v.as_u64()
        .and_then(|v| usize::try_from(v).ok())
        .ok_or_else(|| Error::new("INVALID_DATA", "Expected unsigned integer"))
}
pub fn array(v: &Value) -> Result<&Vec<Value>> {
    v.as_array()
        .ok_or_else(|| Error::new("INVALID_DATA", "Expected array"))
}
pub fn re(pattern: &str) -> regex::Regex {
    thread_local! {static REGEXES:std::cell::RefCell<std::collections::HashMap<String,regex::Regex>>=std::cell::RefCell::new(std::collections::HashMap::new());}
    REGEXES.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some(r) = cache.get(pattern) {
            return r.clone();
        }
        let r = regex::Regex::new(pattern).expect("internal regular expression");
        if cache.len() < 256 {
            cache.insert(pattern.into(), r.clone());
        }
        r
    })
}
