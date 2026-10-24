use crate::{require, Result};
use html5ever::{
    tendril::StrTendril,
    tokenizer::{
        states::RawKind, BufferQueue, CharacterTokens, TagKind, TagToken, Token, TokenSink,
        TokenSinkResult, Tokenizer, TokenizerOpts,
    },
};
use serde_json::{json, Value};
use std::{
    cell::RefCell,
    rc::Rc,
    sync::{Arc, Mutex, OnceLock},
};

/// Immutable response text and its analysis. Clones share only this response;
/// a subsequent HTTP response always constructs a new document.
#[derive(Clone, Debug)]
pub struct Document(Arc<DocumentData>);
#[derive(Debug)]
struct DocumentData {
    source: String,
    facts: OnceLock<Facts>,
    analyzing: Mutex<()>,
}
impl From<String> for Document {
    fn from(source: String) -> Self {
        Self(Arc::new(DocumentData {
            source,
            facts: OnceLock::new(),
            analyzing: Mutex::new(()),
        }))
    }
}
impl From<&str> for Document {
    fn from(source: &str) -> Self {
        source.to_owned().into()
    }
}
impl std::ops::Deref for Document {
    type Target = str;
    fn deref(&self) -> &str {
        &self.0.source
    }
}
impl AsRef<str> for Document {
    fn as_ref(&self) -> &str {
        self
    }
}
impl serde::Serialize for Document {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(self)
    }
}
impl Document {
    /// Move the final response body when its analysis users have finished.
    pub fn into_text(self) -> String {
        match Arc::try_unwrap(self.0) {
            Ok(document) => document.source,
            Err(document) => document.source.clone(),
        }
    }
    pub fn facts(&self) -> Result<&Facts> {
        crate::cpu::check()?;
        if let Some(facts) = self.0.facts.get() {
            return Ok(facts);
        }
        let _guard = crate::cpu::lock(&self.0.analyzing)?;
        if self.0.facts.get().is_none() {
            let facts = Facts::read(self)?;
            crate::cpu::check()?;
            let _ = self.0.facts.set(facts);
        }
        Ok(self.0.facts.get().unwrap())
    }
}

#[derive(Clone, Debug, Default)]
pub struct Facts {
    pub title: String,
    pub prefix: String,
    pub elements: usize,
    pub meta: usize,
    pub scripts: usize,
    pub styles: usize,
    pub links: usize,
    pub images: usize,
    pub forms: usize,
    pub root_attributes: Vec<String>,
    pub body_attributes: Vec<String>,
    pub custom_tags: Vec<String>,
    pub style_texts: Vec<String>,
    pub script_texts: Vec<String>,
    tags: Vec<String>,
    pub extension_comment: bool,
}
fn prefix(f: &mut Facts, value: &str) {
    let mut units: Vec<_> = f.prefix.encode_utf16().collect();
    units.extend(value.encode_utf16());
    units.truncate(500);
    f.prefix = String::from_utf16_lossy(&units)
}
struct Sink(Rc<RefCell<Facts>>);
impl TokenSink for Sink {
    type Handle = ();
    fn process_token(&self, token: Token, _line: u64) -> TokenSinkResult<()> {
        let mut f = self.0.borrow_mut();
        match token {
            TagToken(tag) => {
                let name = tag.name.to_string();
                if tag.kind == TagKind::StartTag {
                    f.elements += 1;
                    f.meta += usize::from(name == "meta");
                    f.scripts += usize::from(name == "script");
                    f.styles += usize::from(
                        name == "style"
                            || (name == "link"
                                && tag.attrs.iter().any(|a| {
                                    a.name.local.as_ref() == "rel"
                                        && a.value
                                            .split_whitespace()
                                            .any(|v| v.eq_ignore_ascii_case("stylesheet"))
                                })),
                    );
                    f.links += usize::from(
                        matches!(name.as_str(), "a" | "area")
                            && tag.attrs.iter().any(|a| a.name.local.as_ref() == "href"),
                    );
                    f.images += usize::from(name == "img");
                    f.forms += usize::from(name == "form");
                    if name == "html" {
                        f.root_attributes =
                            tag.attrs.iter().map(|a| a.name.local.to_string()).collect()
                    }
                    if name == "body" {
                        f.body_attributes =
                            tag.attrs.iter().map(|a| a.name.local.to_string()).collect()
                    }
                    if name.contains('-') {
                        f.custom_tags.push(name.clone())
                    }
                    if f.tags.len() < 3 {
                        let mut text = name.chars().take(3).collect::<String>();
                        for attr in &tag.attrs {
                            text.push('_');
                            text.extend(attr.name.local.chars().take(2));
                        }
                        text.push('>');
                        prefix(&mut f, &text)
                    }
                    if name == "script" {
                        f.script_texts.push(String::new())
                    }
                    if name == "style" {
                        f.style_texts.push(String::new())
                    }
                    if !matches!(
                        name.as_str(),
                        "area"
                            | "base"
                            | "br"
                            | "col"
                            | "embed"
                            | "hr"
                            | "img"
                            | "input"
                            | "link"
                            | "meta"
                            | "param"
                            | "source"
                            | "track"
                            | "wbr"
                    ) && !tag.self_closing
                    {
                        f.tags.push(name.clone())
                    }
                    if matches!(name.as_str(), "script" | "style" | "noscript") {
                        return TokenSinkResult::RawData(if name == "script" {
                            RawKind::ScriptData
                        } else {
                            RawKind::Rawtext
                        });
                    }
                } else if let Some(index) = f.tags.iter().rposition(|n| n == &name) {
                    f.tags.truncate(index)
                }
            }
            CharacterTokens(text) => {
                match f.tags.last().map(String::as_str) {
                    Some("title") => f.title.push_str(&text),
                    Some("script") => {
                        if let Some(s) = f.script_texts.last_mut() {
                            s.push_str(&text)
                        }
                    }
                    Some("style") => {
                        if let Some(s) = f.style_texts.last_mut() {
                            s.push_str(&text)
                        }
                    }
                    _ => {}
                }
                if !text.is_empty() && f.tags.len() < 3 {
                    prefix(&mut f, "-t")
                }
            }
            Token::CommentToken(text) => {
                if f.tags.iter().any(|n| n == "head") && text.contains("UIPATH_EXTENSION_ID:") {
                    f.extension_comment = true
                }
            }
            _ => {}
        }
        TokenSinkResult::Continue
    }
}
impl Facts {
    pub fn read(html: &str) -> Result<Self> {
        crate::cpu::check()?;
        require(html.len() <= 8 * 1024 * 1024, "HTML analysis limit")?;
        let facts = Rc::new(RefCell::new(Self::default()));
        let input = BufferQueue::default();
        input.push_back(StrTendril::from_slice(html));
        let tok = Tokenizer::new(Sink(facts.clone()), TokenizerOpts::default());
        let _ = tok.feed(&input);
        tok.end();
        drop(tok);
        crate::cpu::check()?;
        let value = Rc::try_unwrap(facts)
            .expect("Tokenizer released its sink")
            .into_inner();
        require(
            value.elements <= 100000 && value.tags.len() <= 128,
            "HTML structure limit",
        )?;
        Ok(value)
    }
    pub fn fields(&self, url: &str) -> Value {
        json!({"iukR9":self.prefix,"FFpcc8":self.scripts,"NuzL2":self.styles,"BgKqc3":self.meta,"Zgvv9":title_fingerprint(&self.title),"teFMu4":self.elements,"QAmW3":url})
    }
}

pub fn title_fingerprint(text: &str) -> String {
    let (mut count, mut hash) = (0, 5381u32);
    for unit in text.encode_utf16() {
        count += 1;
        hash = hash.wrapping_mul(33) ^ unit as u32;
    }
    format!("{count}|{hash}")
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn final_text_moves_its_allocation_and_preserves_other_readers() {
        let source = "正文😀".repeat(1024);
        let pointer = source.as_ptr();
        let document = Document::from(source);
        document.facts().unwrap();
        let moved = document.into_text();
        assert_eq!(moved.as_ptr(), pointer);
        let shared = Document::from(moved);
        let reader = shared.clone();
        assert_eq!(shared.into_text(), &*reader);
        let original = reader.into_text();
        assert_eq!(original.as_ptr(), pointer);
    }
    #[test]
    fn concurrent_consumers_share_facts_only_within_the_same_response() {
        let script = "/*中文😀*/let x='value';".repeat(10_000);
        let text = format!("<html><head><title>本轮</title><style>.test{{color:red}}</style><script>{script}</script></head><body data-new>正文</body></html>");
        let document = Document::from(text.clone());
        let start = std::sync::Barrier::new(6);
        let pointers = std::thread::scope(|s| {
            let jobs: Vec<_> = (0..6)
                .map(|_| {
                    let cloned = document.clone();
                    let start = &start;
                    s.spawn(move || {
                        start.wait();
                        cloned.facts().unwrap() as *const Facts as usize
                    })
                })
                .collect();
            jobs.into_iter()
                .map(|j| j.join().unwrap())
                .collect::<Vec<_>>()
        });
        assert!(pointers.iter().all(|p| *p == pointers[0]));
        assert_eq!(document.facts().unwrap().script_texts, [script]);
        assert_eq!(
            document.facts().unwrap().fields("same-url"),
            Facts::read(&text).unwrap().fields("same-url")
        );
        assert_eq!(serde_json::to_value(&document).unwrap(), text);
        let next = Document::from("<title>下一轮</title><script>fresh()</script>");
        assert_eq!(next.facts().unwrap().title, "下一轮");
        assert_eq!(document.facts().unwrap().title, "本轮");
        assert!(!std::ptr::eq(
            document.facts().unwrap(),
            next.facts().unwrap()
        ));
    }
    #[tokio::test]
    async fn cancelled_analysis_waiter_does_not_poison_document() {
        let document = Document::from("<title>fresh</title>");
        let guard = document.0.analyzing.lock().unwrap();
        let cloned = document.clone();
        let error = crate::cpu::run(
            std::time::Instant::now() + std::time::Duration::from_millis(20),
            move || cloned.facts().map(|_| ()),
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, "TIMEOUT");
        drop(guard);
        assert_eq!(document.facts().unwrap().title, "fresh");
    }
}
