//! The same system ICU formatting operations used by the reference model.
use crate::{
    epoch_ms, math,
    models::{has, Context, Results},
    re, require, Error, Result,
};
use serde_json::{json, Value};
use std::{
    collections::HashSet,
    ffi::{c_char, c_void, CStr, CString},
    sync::OnceLock,
};
pub(crate) struct Icu(libloading::Library);
impl Icu {
    pub(crate) fn load() -> Result<&'static Self> {
        static ICU: OnceLock<std::result::Result<Icu, String>> = OnceLock::new();
        let result = ICU.get_or_init(|| {
            #[cfg(windows)]
            let path = std::path::PathBuf::from(
                std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into()),
            )
            .join("System32/icu.dll");
            #[cfg(not(windows))]
            let path = std::path::PathBuf::from("libicui18n.so");
            unsafe {
                match libloading::Library::new(&path) {
                    Ok(library) => Ok(Icu(library)),
                    Err(error) => {
                        #[cfg(not(windows))]
                        for version in (50..=100).rev() {
                            if let Ok(library) =
                                libloading::Library::new(format!("libicui18n.so.{version}"))
                            {
                                return Ok(Icu(library));
                            }
                        }
                        Err(error.to_string())
                    }
                }
            }
        });
        result
            .as_ref()
            .map_err(|e| Error::new("LOCALE_UNAVAILABLE", e.clone()))
    }
    unsafe fn function<T: Copy>(&self, name: &[u8]) -> Result<T> {
        if let Ok(symbol) = self.0.get::<T>(name) {
            return Ok(*symbol);
        }
        // Linux distributions export versioned ICU symbols; Windows ICU uses
        // unversioned names. Resolve the installed ABI instead of pinning one.
        #[cfg(not(windows))]
        for version in (50..=100).rev() {
            let symbol = format!(
                "{}_{version}\0",
                std::str::from_utf8(name.strip_suffix(&[0]).unwrap_or(name)).unwrap()
            );
            if let Ok(symbol) = self.0.get::<T>(symbol.as_bytes()) {
                return Ok(*symbol);
            }
        }
        Err(Error::new(
            "LOCALE_UNAVAILABLE",
            format!("Missing ICU operation {}", String::from_utf8_lossy(name)),
        ))
    }
    fn locale(&self) -> Result<String> {
        unsafe {
            let f: unsafe extern "C" fn() -> *const c_char = self.function(b"uloc_getDefault\0")?;
            let p = f();
            require(!p.is_null(), "ICU locale")?;
            Ok(CStr::from_ptr(p)
                .to_string_lossy()
                .split('@')
                .next()
                .unwrap()
                .replace('_', "-"))
        }
    }
    pub(crate) fn format(
        &self,
        locale: &str,
        zone: &str,
        timestamp: f64,
        language: &str,
        words: &[String],
        number: f64,
    ) -> Result<Value> {
        require(
            re(r"^[A-Za-z]{2,3}(?:[-_][A-Za-z0-9]{2,8})*$").is_match(locale)
                && re(r"^[A-Za-z_]+(?:/[A-Za-z_+-]+)*$").is_match(zone)
                && re(r"^[a-z]{2,3}-[A-Z]{2}$").is_match(language)
                && !words.is_empty()
                && words.len() <= 8
                && timestamp.is_finite()
                && number.is_finite(),
            "Locale operands",
        )?;
        let locale = CString::new(locale.replace('-', "_"))
            .map_err(|_| Error::new("INVALID_DATA", "Locale string"))?;
        let timezone: Vec<u16> = zone.encode_utf16().chain([0]).collect();
        let pattern: Vec<u16> = "LLLL zzzz".encode_utf16().chain([0]).collect();
        let mut out = json!({});
        unsafe {
            let open: unsafe extern "C" fn(
                i32,
                i32,
                *const c_char,
                *const u16,
                i32,
                *const u16,
                i32,
                *mut i32,
            ) -> *mut c_void = self.function(b"udat_open\0")?;
            let format: unsafe extern "C" fn(
                *mut c_void,
                f64,
                *mut u16,
                i32,
                *mut c_void,
                *mut i32,
            ) -> i32 = self.function(b"udat_format\0")?;
            let close: unsafe extern "C" fn(*mut c_void) = self.function(b"udat_close\0")?;
            let mut status = 0;
            let handle = open(
                -2,
                -2,
                locale.as_ptr(),
                timezone.as_ptr(),
                -1,
                pattern.as_ptr(),
                -1,
                &mut status,
            );
            check(status)?;
            require(!handle.is_null(), "ICU date handle")?;
            let handle = Handle {
                pointer: handle,
                close,
            };
            let mut buffer = vec![0u16; 2048];
            let count = format(
                handle.pointer,
                timestamp,
                buffer.as_mut_ptr(),
                2048,
                std::ptr::null_mut(),
                &mut status,
            );
            out["mMAX8"] = json!(output(&buffer, count, status)?);
            let open: unsafe extern "C" fn(*const c_char, i32, *mut i32) -> *mut c_void =
                self.function(b"uldn_open\0")?;
            let format: unsafe extern "C" fn(
                *mut c_void,
                *const c_char,
                *mut u16,
                i32,
                *mut i32,
            ) -> i32 = self.function(b"uldn_localeDisplayName\0")?;
            let close: unsafe extern "C" fn(*mut c_void) = self.function(b"uldn_close\0")?;
            status = 0;
            let handle = open(locale.as_ptr(), 0, &mut status);
            check(status)?;
            require(!handle.is_null(), "ICU display handle")?;
            let handle = Handle {
                pointer: handle,
                close,
            };
            let language = CString::new(language.replace('-', "_")).unwrap();
            let count = format(
                handle.pointer,
                language.as_ptr(),
                buffer.as_mut_ptr(),
                2048,
                &mut status,
            );
            out["NzGy3"] = json!(output(&buffer, count, status)?);
            let open: unsafe extern "C" fn(*const c_char, i32, i32, *mut i32) -> *mut c_void =
                self.function(b"ulistfmt_openForType\0")?;
            let format: unsafe extern "C" fn(
                *mut c_void,
                *const *const u16,
                *const i32,
                i32,
                *mut u16,
                i32,
                *mut i32,
            ) -> i32 = self.function(b"ulistfmt_format\0")?;
            let close: unsafe extern "C" fn(*mut c_void) = self.function(b"ulistfmt_close\0")?;
            status = 0;
            let handle = open(locale.as_ptr(), 1, 0, &mut status);
            check(status)?;
            require(!handle.is_null(), "ICU list handle")?;
            let handle = Handle {
                pointer: handle,
                close,
            };
            let words: Vec<Vec<u16>> = words
                .iter()
                .map(|s| s.encode_utf16().chain([0]).collect())
                .collect();
            let pointers: Vec<_> = words.iter().map(|s| s.as_ptr()).collect();
            let lengths = vec![-1i32; words.len()];
            let count = format(
                handle.pointer,
                pointers.as_ptr(),
                lengths.as_ptr(),
                words.len() as i32,
                buffer.as_mut_ptr(),
                2048,
                &mut status,
            );
            out["FNvn2"] = json!(output(&buffer, count, status)?);
            let open: unsafe extern "C" fn(
                i32,
                *const u16,
                i32,
                *const c_char,
                *mut c_void,
                *mut i32,
            ) -> *mut c_void = self.function(b"unum_open\0")?;
            let format: unsafe extern "C" fn(
                *mut c_void,
                f64,
                *mut u16,
                i32,
                *mut c_void,
                *mut i32,
            ) -> i32 = self.function(b"unum_formatDouble\0")?;
            let close: unsafe extern "C" fn(*mut c_void) = self.function(b"unum_close\0")?;
            status = 0;
            let handle = open(
                15,
                std::ptr::null(),
                0,
                locale.as_ptr(),
                std::ptr::null_mut(),
                &mut status,
            );
            check(status)?;
            require(!handle.is_null(), "ICU number handle")?;
            let handle = Handle {
                pointer: handle,
                close,
            };
            let count = format(
                handle.pointer,
                number,
                buffer.as_mut_ptr(),
                2048,
                std::ptr::null_mut(),
                &mut status,
            );
            out["HxIG7"] = json!(output(&buffer, count, status)?);
        }
        Ok(out)
    }
}
struct Handle {
    pointer: *mut c_void,
    close: unsafe extern "C" fn(*mut c_void),
}
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe { (self.close)(self.pointer) }
    }
}
fn check(status: i32) -> Result<()> {
    require(status <= 0, "ICU rejected locale operands")
}
fn output(buffer: &[u16], count: i32, status: i32) -> Result<String> {
    check(status)?;
    require(
        count >= 0 && (count as usize) < buffer.len(),
        "ICU output bounds",
    )?;
    String::from_utf16(&buffer[..count as usize])
        .map_err(|_| Error::new("INVALID_DATA", "ICU Unicode output"))
}
pub fn calculate(c: &Context<'_>, results: &mut Results) -> Result<()> {
    let tag = c.catalog.modules.iter().find_map(|m| {
        if c.response
            .between(m.start, m.end)
            .iter()
            .any(|l| l.value == "zJGo6")
        {
            math::seeds(c.response, m, c.plan).ok().map(|x| x.2)
        } else {
            None
        }
    });
    for m in &c.catalog.modules {
        let entered = epoch_ms();
        let literals = c.response.between(m.start, m.end);
        let values: HashSet<_> = literals.iter().map(|l| l.value.as_str()).collect();
        if !has(&values, &["XwYHI5", "Ecdf9", "yGyw0", "qTcMk3"]) {
            continue;
        }
        let tag = tag.ok_or_else(|| Error::new("INVALID_DATA", "Locale numeric tag"))?;
        let f = &c.plan.format;
        let raw = &c.response.raw;
        let mut numbers = Vec::new();
        for position in m.start..m.end.saturating_sub(11) {
            let key = f.key(f.byte(raw[position], f.state_step), f.state_step);
            if f.byte(raw[position + c.plan.kind_slot()?], key) != tag {
                continue;
            }
            let number =
                f64::from_be_bytes(std::array::from_fn(|i| f.byte(raw[position + 3 + i], key)));
            if (1e11..=1e13).contains(&number) || (1e15..=1e18).contains(&number) {
                numbers.push((position, number));
            }
        }
        let (timestamp, amount, language, words) = operands(&literals, &numbers)?;
        let icu = Icu::load()?;
        let locale = std::env::var("CF_GATEWAY_LOCALE").unwrap_or(icu.locale()?);
        let zone = std::env::var("CF_GATEWAY_TIMEZONE").unwrap_or_else(|_| "Asia/Shanghai".into());
        let mut fields = icu.format(&locale, &zone, timestamp, &language, &words, amount)?;
        fields["XwYHI5"] = json!("zh-CN");
        fields["Ecdf9"] = json!(["zh-CN", "zh", "en"]);
        fields["yGyw0"] = json!([locale]);
        fields["qTcMk3"] = json!("en-us");
        results.insert(&m.id, "native-locale", fields, entered);
    }
    Ok(())
}

pub(crate) fn operands(
    input: &[&crate::data::Literal],
    numbers: &[(usize, f64)],
) -> Result<(f64, f64, String, Vec<String>)> {
    let mut literals = input.to_vec();
    literals.sort_by_key(|l| l.start);
    let values = literals
        .iter()
        .map(|l| l.value.as_str())
        .collect::<HashSet<_>>();
    let after = |option: &str| -> Result<f64> {
        let anchors: Vec<_> = (0..literals.len().saturating_sub(2))
            .filter(|i| literals[*i].value == option && literals[*i + 1].value == "long")
            .collect();
        require(anchors.len() == 1, "Locale option group")?;
        let i = anchors[0] + 1;
        let mut values: Vec<_> = numbers
            .iter()
            .filter(|(p, _)| literals[i].start < *p && *p < literals[i + 1].start)
            .map(|(_, n)| *n)
            .collect();
        values.sort_by(f64::total_cmp);
        values.dedup();
        require(values.len() == 1, "Locale numeric operand")?;
        Ok(values[0])
    };
    let timestamp = after("timeZoneName")?;
    let amount = after("compactDisplay")?;
    let languages: Vec<_> = values
        .iter()
        .filter(|s| re(r"^[a-z]{2,3}-[A-Z]{2}$").is_match(s))
        .collect();
    let lists: Vec<_> = (0..literals.len().saturating_sub(2))
        .filter(|i| literals[*i].value == "disjunction")
        .collect();
    require(
        languages.len() == 1 && lists.len() == 1,
        "Locale language/list operands",
    )?;
    let i = lists[0];
    let text = &literals[i + 1].value;
    let separator = &literals[i + 2].value;
    require(
        re(r"^[A-Za-z0-9]{3,512}$").is_match(text) && re(r"^[A-Za-z]$").is_match(separator),
        "Locale list format",
    )?;
    let words = text.split(separator).map(String::from).collect::<Vec<_>>();
    require(
        (1..=8).contains(&words.len())
            && words.iter().all(|s| !s.is_empty())
            && (1e11..=1e13).contains(&timestamp)
            && (1e15..=1e18).contains(&amount),
        "Locale task operand bounds",
    )?;
    Ok((timestamp, amount, languages[0].to_string(), words))
}
