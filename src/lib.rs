#![recursion_limit = "512"]
pub mod benchmark;
pub mod catalog;
pub mod compiler;
pub mod compiler_profiles;
pub mod completion;
pub mod constants;
pub mod css;
pub mod data;
pub mod declared;
pub mod embedded;
pub mod engine;
pub mod html;
pub mod http_tasks;
pub mod identity;
pub mod jsd;
pub mod locale;
pub mod math;
pub mod model_geometry;
pub mod models;
pub mod native_input;
pub mod network;
pub mod operands;
pub mod raster;
pub mod semantics;
pub mod service;
pub mod sources;
pub mod static_data;
pub mod surface;
pub mod version;
pub mod web;
pub mod wire;

#[cfg(test)]
mod migration_tests;
#[cfg(test)]
mod native_regression;

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
