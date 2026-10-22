use crate::{re, require, Error, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};
use tree_sitter::{Node, Parser};

pub fn unquote(source: &str) -> Result<String> {
    let mut chars = source.chars();
    let quote = chars
        .next()
        .ok_or_else(|| Error::new("INVALID_DATA", "Empty string"))?;
    require(
        source.len() >= 2 && matches!(quote, '\'' | '"' | '`') && source.ends_with(quote),
        "String delimiter",
    )?;
    let mut chars = source[quote.len_utf8()..source.len() - quote.len_utf8()]
        .chars()
        .peekable();
    let mut result = Vec::<u16>::new();
    while let Some(c) = chars.next() {
        require(
            !(quote == '`' && c == '$' && chars.peek() == Some(&'{')),
            "Template interpolation is not data",
        )?;
        if c != '\\' {
            result.extend(c.encode_utf16(&mut [0; 2]).iter().copied());
            continue;
        }
        let c = chars
            .next()
            .ok_or_else(|| Error::new("INVALID_DATA", "String escape"))?;
        let n = match c {
            'n' => 10,
            'r' => 13,
            't' => 9,
            'b' => 8,
            'f' => 12,
            'v' => 11,
            '0' => 0,
            'x' | 'u' => {
                let count = if c == 'x' { 2 } else { 4 };
                let digits: String = chars.by_ref().take(count).collect();
                require(digits.len() == count, "Short string escape")?;
                u16::from_str_radix(&digits, 16)
                    .map_err(|_| Error::new("INVALID_DATA", "Hex escape"))?
            }
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                continue;
            }
            '\n' => continue,
            _ => {
                result.extend(c.encode_utf16(&mut [0; 2]).iter().copied());
                continue;
            }
        };
        result.push(n)
    }
    String::from_utf16(&result)
        .map_err(|_| Error::new("INVALID_DATA", "Unpaired configuration string"))
}
fn literal(node: Node<'_>, source: &str, depth: usize) -> Result<Value> {
    require(depth <= 32, "Configuration depth")?;
    let text = &source[node.byte_range()];
    Ok(match node.kind() {
        "string" | "template_string" => json!(unquote(text)?),
        "true" => json!(true),
        "false" => json!(false),
        "null" => Value::Null,
        "number" => {
            if let Some(hex) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
                json!(u64::from_str_radix(hex, 16)
                    .map_err(|_| Error::new("INVALID_DATA", "Configuration integer"))?)
            } else {
                serde_json::from_str(text)?
            }
        }
        "unary_expression" => {
            require(text.starts_with('-'), "Unsupported unary expression")?;
            let value = literal(node.named_child(0).unwrap(), source, depth + 1)?;
            json!(-value
                .as_f64()
                .ok_or_else(|| Error::new("INVALID_DATA", "Unary number"))?)
        }
        "array" => {
            let mut cursor = node.walk();
            let mut out = Vec::new();
            for n in node.named_children(&mut cursor) {
                if n.kind() != "comment" {
                    out.push(literal(n, source, depth + 1)?)
                }
            }
            Value::Array(out)
        }
        "object" => {
            let mut out = serde_json::Map::new();
            let mut cursor = node.walk();
            for pair in node.named_children(&mut cursor) {
                if pair.kind() == "comment" {
                    continue;
                }
                require(pair.kind() == "pair", "Executable object member")?;
                let key = pair.child_by_field_name("key").unwrap();
                let key = if matches!(key.kind(), "property_identifier" | "identifier") {
                    source[key.byte_range()].to_string()
                } else {
                    unquote(&source[key.byte_range()])?
                };
                require(!out.contains_key(&key), "Duplicate configuration field")?;
                let member = pair.child_by_field_name("value").unwrap();
                // A callback declaration is not configuration data and is
                // never executed. Other malformed values must not disappear.
                if key == "callback"
                    && matches!(member.kind(), "function_expression" | "arrow_function")
                {
                    continue;
                }
                out.insert(key, literal(member, source, depth + 1)?);
            }
            Value::Object(out)
        }
        "call_expression" => {
            let pattern = re(r"^btoa\(\s*String\(\s*([0-9]{1,13})\s*\)\s*\)$");
            let c = pattern
                .captures(text)
                .ok_or_else(|| Error::new("INVALID_DATA", "Calls are not configuration data"))?;
            json!(STANDARD.encode(
                c[1].parse::<u64>()
                    .map_err(|_| Error::new("INVALID_DATA", "Timestamp"))?
                    .to_string()
            ))
        }
        _ => {
            return Err(Error::new(
                "INVALID_DATA",
                "Executable configuration member",
            ))
        }
    })
}
pub fn assignment(source: &str, name: &str) -> Result<Option<Value>> {
    require(source.len() <= 8 * 1024 * 1024, "Script limit")?;
    if !source.contains(name) {
        return Ok(None);
    }
    if source.trim_start().starts_with('<') {
        let scripts = crate::html::Facts::read(source)?.script_texts;
        let mut values = Vec::new();
        for script in scripts {
            if let Some(value) = assignment(&script, name)? {
                values.push(value);
            }
        }
        require(values.len() <= 1, "Ambiguous HTML configuration")?;
        return Ok(values.pop());
    }
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_javascript::LANGUAGE.into())
        .map_err(|e| Error::new("PARSER_ERROR", e.to_string()))?;
    let tree = parser
        .parse(source, None)
        .ok_or_else(|| Error::new("PARSER_ERROR", "Cannot parse configuration"))?;
    let mut pending = vec![tree.root_node()];
    let mut matches = Vec::new();
    while let Some(node) = pending.pop() {
        if node.kind() == "assignment_expression" {
            if let (Some(left), Some(right)) = (
                node.child_by_field_name("left"),
                node.child_by_field_name("right"),
            ) {
                let target = &source[left.byte_range()];
                if target == name || target == format!("window.{name}") {
                    matches.push(literal(right, source, 0)?);
                    continue;
                }
                if name == "__CF$cv$params"
                    && target.ends_with(".innerHTML")
                    && right.kind() == "string"
                {
                    let nested = unquote(&source[right.byte_range()])?;
                    if nested.trim_start().starts_with("window.__CF$cv$params=")
                        || nested.trim_start().starts_with("__CF$cv$params=")
                    {
                        if let Some(value) = assignment(&nested, name)? {
                            matches.push(value);
                        }
                    }
                }
            }
        }
        let mut cursor = node.walk();
        pending.extend(node.named_children(&mut cursor));
    }
    require(matches.len() <= 1, "Ambiguous configuration assignment")?;
    Ok(matches.pop())
}

pub fn origin(url: &str) -> Result<String> {
    let u = validate_url(url)?;
    Ok(u.origin().ascii_serialization())
}
pub fn literal_data(source: &str) -> Result<Value> {
    require(source.len() <= 1024 * 1024, "Literal input bounds")?;
    let wrapped = format!("({source})");
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_javascript::LANGUAGE.into())
        .unwrap();
    let tree = parser
        .parse(&wrapped, None)
        .ok_or_else(|| Error::new("INVALID_DATA", "Literal parse"))?;
    require(!tree.root_node().has_error(), "Invalid data literal syntax")?;
    require(
        !crate::constants::walk(tree.root_node(), true)
            .iter()
            .any(|n| {
                matches!(
                    n.kind(),
                    "call_expression"
                        | "function_expression"
                        | "function_declaration"
                        | "arrow_function"
                        | "method_definition"
                )
            }),
        "Executable values are not literal data",
    )?;
    let node = tree
        .root_node()
        .named_child(0)
        .and_then(|n| n.named_child(0))
        .ok_or_else(|| Error::new("INVALID_DATA", "Literal absent"))?;
    literal(crate::constants::unwrap(node), &wrapped, 0)
}
/// Read only data members of a configuration object; executable values remain
/// inert and are omitted. Duplicate objects/keys remain errors.
pub(crate) fn object_data(source: &str, name: &str) -> Result<Option<Value>> {
    require(source.len() <= 8 * 1024 * 1024, "Script limit")?;
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_javascript::LANGUAGE.into())
        .unwrap();
    let tree = parser
        .parse(source, None)
        .ok_or_else(|| Error::new("INVALID_DATA", "Object parse"))?;
    let mut found = Vec::new();
    for node in crate::constants::walk(tree.root_node(), true)
        .into_iter()
        .filter(|n| n.kind() == "assignment_expression")
    {
        let left = crate::constants::child(node, "left")?;
        let target = &source[left.byte_range()];
        if target != name && target != format!("window.{name}") {
            continue;
        }
        let object = crate::constants::child(node, "right")?;
        require(object.kind() == "object", "Configuration is not an object")?;
        let mut out = serde_json::Map::new();
        let mut seen = std::collections::HashSet::new();
        for pair in crate::constants::children(object) {
            if pair.kind() == "comment" {
                continue;
            }
            require(pair.kind() == "pair", "Invalid data member")?;
            let key = crate::constants::child(pair, "key")?;
            let key = if matches!(key.kind(), "property_identifier" | "identifier") {
                source[key.byte_range()].to_string()
            } else {
                unquote(&source[key.byte_range()])?
            };
            require(seen.insert(key.clone()), "Duplicate configuration field")?;
            if let Ok(value) =
                literal_data(&source[crate::constants::child(pair, "value")?.byte_range()])
            {
                out.insert(key, value);
            }
        }
        found.push(json!(out));
    }
    require(found.len() <= 1, "Ambiguous configuration object")?;
    Ok(found.pop())
}
pub fn validate_url(url: &str) -> Result<url::Url> {
    let invalid = || {
        let mut e = Error::new(
            "INVALID_URL",
            "需要有效的 HTTP/HTTPS 网址，不接受网址中的账号密码",
        );
        e.status = 400;
        e
    };
    if url.len() > 8192 || url.chars().any(|c| c < ' ') {
        return Err(invalid());
    }
    let mut u = url::Url::parse(url).map_err(|_| invalid())?;
    if !(matches!(u.scheme(), "http" | "https")
        && u.host_str().is_some()
        && u.username().is_empty()
        && u.password().is_none())
    {
        return Err(invalid());
    }
    u.set_fragment(None);
    Ok(u)
}
#[derive(Clone, Debug)]
pub struct Challenge {
    pub kind: String,
    pub config: Value,
    pub script: Option<String>,
}
impl Challenge {
    pub fn summary(&self) -> Value {
        json!({"type":self.kind,"ray":self.config.get("cRay").or_else(||self.config.get("r")),"configFields":self.config.as_object().map(|o|o.keys().collect::<Vec<_>>()),"script":self.script.as_ref().and_then(|s|url::Url::parse(s).ok()).map(|u|u.path().to_string())})
    }
}
pub fn recognize(
    url: &str,
    status: u16,
    headers: &serde_json::Map<String, Value>,
    html: &str,
) -> Result<Challenge> {
    let content = headers
        .get("content-type")
        .and_then(Value::as_str)
        .unwrap_or("");
    let empty = || Challenge {
        kind: "none".into(),
        config: json!({}),
        script: None,
    };
    if !content.is_empty()
        && !(["html", "javascript", "text/plain"]
            .iter()
            .any(|s| content.contains(s)))
    {
        return Ok(empty());
    }
    let mitigated = headers.get("cf-mitigated").and_then(Value::as_str) == Some("challenge");
    let config = match assignment(html, "_cf_chl_opt") {
        Ok(Some(v)) => v,
        Ok(None) => json!({}),
        Err(e) => {
            if mitigated {
                return Err(e);
            }
            json!({})
        }
    };
    if mitigated
        || matches!(
            config["cType"].as_str(),
            Some("managed" | "interactive" | "non-interactive" | "precursor_interstitial")
        )
    {
        let pat = re(r#"(?:\w+\.src|\bsrc)\s*=\s*['"](/cdn-cgi/challenge-platform/[^'"]+)['"]"#);
        let script = pat
            .captures_iter(html)
            .map(|c| c[1].to_string())
            .find(|v| v.contains("/orchestrate/"));
        let script = script
            .map(|s| {
                validate_url(url)?
                    .join(&s)
                    .map(|u| u.to_string())
                    .map_err(|_| Error::new("INVALID_DATA", "Script URL"))
            })
            .transpose()?;
        return Ok(Challenge {
            kind: "managed".into(),
            config,
            script,
        });
    }
    if let Some(widget) = crate::embedded::recognize(url, html)? {
        return Ok(Challenge {
            kind: "embedded-turnstile".into(),
            config: json!({"sitekey":widget.sitekey,"adapter":"69shuba-search"}),
            script: Some(widget.api_url),
        });
    }
    if let Some(config) = assignment(html, "__CF$cv$params")? {
        if config.get("r").is_some() && config.get("t").is_some() {
            let doc = scraper::Html::parse_document(html);
            let title_selector = scraper::Selector::parse("title").unwrap();
            let title = doc
                .select(&title_selector)
                .next()
                .map(|e| e.text().collect::<String>())
                .unwrap_or_default();
            let selector = scraper::Selector::parse("body").unwrap();
            let count = doc
                .select(&selector)
                .next()
                .map(|e| {
                    e.descendants()
                        .filter_map(|n| n.value().as_text().map(|t| (n, t)))
                        .filter(|(n, _)| {
                            !n.ancestors().any(|a| {
                                a.value().as_element().is_some_and(|e| {
                                    matches!(
                                        e.name(),
                                        "script" | "style" | "noscript" | "iframe" | "template"
                                    )
                                })
                            })
                        })
                        .map(|(_, t)| t.trim().chars().count())
                        .sum::<usize>()
                })
                .unwrap_or(0);
            let visible = (200..300).contains(&status) || status == 404;
            let heading = scraper::Selector::parse("body h1, body h2, body h3").unwrap();
            let heading_count = doc
                .select(&heading)
                .map(|e| e.text().collect::<String>().trim().chars().count())
                .sum::<usize>();
            let visible=visible&&(count>=256||(!title.trim().is_empty()&&count>=16&&heading_count>=8))&&!re("(?i)just a moment|attention required|access denied|please wait|verify.{0,30}human|checking.{0,30}browser").is_match(&title);
            return Ok(Challenge {
                kind: if visible { "jsd-background" } else { "jsd" }.into(),
                config,
                script: if visible {
                    None
                } else {
                    Some(origin(url)? + "/cdn-cgi/challenge-platform/scripts/jsd/main.js")
                },
            });
        }
    }
    if status == 403 && html.to_lowercase().contains("cloudflare") && html.contains("cf-error") {
        return Ok(Challenge {
            kind: "blocked".into(),
            ..empty()
        });
    }
    Ok(empty())
}
