use crate::{require, Result};
use html5ever::{
    tendril::StrTendril,
    tokenizer::{
        states::RawKind, BufferQueue, CharacterTokens, TagKind, TagToken, Token, TokenSink,
        TokenSinkResult, Tokenizer, TokenizerOpts,
    },
};
use serde_json::{json, Value};
use std::{cell::RefCell, rc::Rc};

#[derive(Clone, Default)]
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
        require(html.len() <= 8 * 1024 * 1024, "HTML analysis limit")?;
        let facts = Rc::new(RefCell::new(Self::default()));
        let input = BufferQueue::default();
        input.push_back(StrTendril::from_slice(html));
        let tok = Tokenizer::new(Sink(facts.clone()), TokenizerOpts::default());
        let _ = tok.feed(&input);
        tok.end();
        let value = facts.borrow().clone();
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
