//! Explicit site adapters for embedded widgets. Callback JavaScript is never run.
use crate::{re, require, static_data, Error, Result};
use scraper::{Html, Selector};
use serde_json::Value;

#[derive(Clone, Debug)]
pub struct Widget {
    pub sitekey: String,
    pub api_url: String,
    pub verify_url: String,
}

/// A logical search URL keeps replay inputs in the existing read-only fetch
/// interface. Only this known site's read-only search form is POSTed; arbitrary
/// methods, bodies, credential forms and destinations are not exposed.
pub fn search_form(url: &str) -> Result<Option<Vec<u8>>> {
    let page = static_data::validate_url(url)?;
    if page.scheme() != "https"
        || page.host_str() != Some("www.69shuba.com")
        || page.port().is_some()
        || page.path() != "/modules/article/search.php"
    {
        return Ok(None);
    }
    let mut fields = Vec::new();
    let mut names = std::collections::HashSet::new();
    for pair in page
        .query()
        .unwrap_or("")
        .split('&')
        .filter(|s| !s.is_empty())
    {
        let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
        if name.starts_with("__cf_chl_") {
            continue;
        }
        if name == "searchid" {
            return Ok(None);
        }
        require(
            matches!(name, "searchkey" | "searchtype" | "page") && names.insert(name),
            "Unsupported search form fields",
        )?;
        require(
            re(r"^(?:[A-Za-z0-9_.~+*-]|%[a-fA-F0-9]{2})*$").is_match(value),
            "Search form encoding",
        )?;
        fields.push(pair);
    }
    if !names.contains("searchkey") {
        return Ok(None);
    }
    // Do not decode/re-encode: 69shuba submits GBK, not UTF-8.
    Ok(Some(fields.join("&").into_bytes()))
}

pub fn recognize(url: &str, html: &str) -> Result<Option<Widget>> {
    recognize_document(url, &html.into())
}
pub(crate) fn recognize_document(
    url: &str,
    html: &crate::html::Document,
) -> Result<Option<Widget>> {
    crate::cpu::check()?;
    let page = static_data::validate_url(url)?;
    if page.scheme() != "https"
        || page.host_str() != Some("www.69shuba.com")
        || page.port().is_some()
        || page.path() != "/modules/article/search.php"
        || !html.contains("verify_token")
    {
        return Ok(None);
    }
    let doc = Html::parse_document(html);
    if doc
        .select(&Selector::parse("#cfts").unwrap())
        .next()
        .is_none()
    {
        return Ok(None);
    }
    let scripts = Selector::parse("script[src]").unwrap();
    let apis: Vec<_> = doc
        .select(&scripts)
        .filter_map(|e| {
            let u = page.join(e.value().attr("src")?).ok()?;
            (u.scheme() == "https"
                && u.host_str() == Some("challenges.cloudflare.com")
                && u.path() == "/turnstile/v0/api.js"
                && u.username().is_empty()
                && u.password().is_none()
                && u.port().is_none())
            .then_some(u.to_string())
        })
        .collect();
    require(apis.len() == 1, "Embedded widget API declaration")?;
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_javascript::LANGUAGE.into())
        .unwrap();
    let mut keys = Vec::new();
    for script in &html.facts()?.script_texts {
        if !script.contains("turnstile.render") {
            continue;
        }
        let tree = crate::cpu::parse(&mut parser, script)?;
        for node in crate::constants::walk(tree.root_node(), true) {
            crate::cpu::check()?;
            if node.kind() != "call_expression" {
                continue;
            }
            let Some(fun) = node.child_by_field_name("function") else {
                continue;
            };
            if &script[fun.byte_range()] != "turnstile.render" {
                continue;
            }
            let args = crate::constants::children(crate::constants::child(node, "arguments")?);
            require(
                args.len() == 2
                    && static_data::literal_data(&script[args[0].byte_range()])? == "#cfts",
                "Widget container",
            )?;
            require(args[1].kind() == "object", "Widget options object")?;
            let mut names = std::collections::HashSet::new();
            let mut sitekey = None;
            for pair in crate::constants::children(args[1]) {
                if pair.kind() == "comment" {
                    continue;
                }
                require(pair.kind() == "pair", "Widget option pair")?;
                let name = crate::constants::child(pair, "key")?;
                let name = if name.kind() == "property_identifier" {
                    script[name.byte_range()].to_string()
                } else {
                    static_data::unquote(&script[name.byte_range()])?
                };
                require(names.insert(name.clone()), "Duplicate widget option")?;
                let value = crate::constants::child(pair, "value")?;
                match name.as_str() {
                    "sitekey" => {
                        sitekey = Some(static_data::literal_data(&script[value.byte_range()])?)
                    }
                    "callback" => require(
                        matches!(value.kind(), "function_expression" | "arrow_function"),
                        "Widget callback declaration",
                    )?,
                    _ => {
                        return Err(Error::new(
                            "EMBEDDED_FORMAT_UNSUPPORTED",
                            "未支持的嵌入式组件参数",
                        ))
                    }
                }
            }
            require(
                names.len() == 2 && names.contains("callback"),
                "Widget options",
            )?;
            let sitekey =
                sitekey.ok_or_else(|| Error::new("INVALID_DATA", "Missing widget sitekey"))?;
            let key = crate::string(&sitekey)?;
            require(
                re(r"^0x4[A-Za-z0-9_-]{15,40}$").is_match(key),
                "Widget sitekey",
            )?;
            keys.push(key.to_string());
        }
        // This adapter implements only the site's documented JSON/cookie callback.
        require(
            re(r#"url\s*:\s*['\"]/verify\.php['\"]"#).is_match(&script)
                && re(r#"type\s*:\s*['\"]POST['\"]"#).is_match(&script)
                && re(r"document\.cookie\s*=\s*response\.data").is_match(&script)
                && script.contains("window.location.reload"),
            "Unsupported widget callback",
        )?;
    }
    require(keys.len() == 1, "Ambiguous embedded widget")?;
    Ok(Some(Widget {
        sitekey: keys.remove(0),
        api_url: apis[0].clone(),
        verify_url: page.join("/verify.php").unwrap().into(),
    }))
}

pub fn api_build(url: &str) -> Result<String> {
    let u = static_data::validate_url(url)?;
    require(
        u.scheme() == "https"
            && u.host_str() == Some("challenges.cloudflare.com")
            && u.port().is_none(),
        "Foreign widget API",
    )?;
    re(r"^/turnstile/v0/b/([a-f0-9]{12})/api\.js$")
        .captures(u.path())
        .map(|c| c[1].to_string())
        .ok_or_else(|| Error::new("EMBEDDED_BUILD_UNSUPPORTED", "无法确认嵌入式验证组件版本"))
}

pub fn response_cookie(response: &Value) -> Result<String> {
    require(
        response["code"] == 200 || response["code"] == "200",
        "Site rejected embedded verification",
    )?;
    let cookie = crate::string(&response["data"])?;
    require(
        !cookie.is_empty()
            && cookie.len() <= 4096
            && !cookie.chars().any(|c| c < ' ' || c == '\u{7f}'),
        "Invalid site verification cookie",
    )?;
    let (name, value) = cookie
        .split(';')
        .next()
        .unwrap()
        .split_once('=')
        .ok_or_else(|| Error::new("INVALID_DATA", "Site verification cookie pair"))?;
    require(
        re(r"^[A-Za-z0-9_-]{1,128}$").is_match(name.trim())
            && !value.is_empty()
            && !name.trim().to_ascii_lowercase().starts_with("__cf")
            && !name.trim().eq_ignore_ascii_case("cf_clearance"),
        "Invalid site verification cookie name",
    )?;
    for attr in cookie.split(';').skip(1) {
        if let Some((key, value)) = attr.trim().split_once('=') {
            if key.eq_ignore_ascii_case("domain") {
                require(
                    matches!(
                        value
                            .trim()
                            .trim_start_matches('.')
                            .to_ascii_lowercase()
                            .as_str(),
                        "69shuba.com" | "www.69shuba.com"
                    ),
                    "Foreign verification cookie domain",
                )?;
            }
        }
    }
    Ok(cookie.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    const PAGE: &str = r#"<script src="https://challenges.cloudflare.com/turnstile/v0/api.js?onload=ready"></script><div id="cfts"></div><script>turnstile.render('#cfts',{sitekey:'0x4AAAAAAAAAAAAAAAAAAAAA',callback:function(token){$.ajax({url:'/verify.php',type:'POST',data:{verify_token:token},success:function(response){document.cookie=response.data;window.location.reload();}});}});</script>"#;
    const URL: &str = "https://www.69shuba.com/modules/article/search.php?searchkey=test";
    #[test]
    fn recognizes_only_the_supported_site_flow_without_running_callbacks() {
        let w = recognize(URL, PAGE).unwrap().unwrap();
        assert_eq!(w.verify_url, "https://www.69shuba.com/verify.php");
        assert!(
            recognize("https://other.example/modules/article/search.php", PAGE)
                .unwrap()
                .is_none()
        );
        assert!(recognize("https://www.69shuba.com/book/1.htm", PAGE)
            .unwrap()
            .is_none());
        assert!(recognize(
            URL,
            &PAGE.replace("'/verify.php'", "'https://other.example/'")
        )
        .is_err());
        assert!(recognize(
            URL,
            &PAGE.replace("sitekey:'", "action:dangerous(),sitekey:'")
        )
        .is_err());
        assert!(recognize(URL, &format!("{PAGE}{PAGE}")).is_err());
    }
    #[test]
    fn cookie_must_be_successful_bounded_and_site_scoped() {
        assert!(response_cookie(&serde_json::json!({"code":200,"data":"verify=yes; Path=/; Domain=.69shuba.com; Secure"})).is_ok());
        for value in [
            "",
            "verify=x\r\nX-Test: bad",
            "cf_clearance=replace",
            "verify=x; Domain=other.example",
            "not-a-cookie",
        ] {
            assert!(response_cookie(&serde_json::json!({"code":200,"data":value})).is_err());
        }
        assert!(response_cookie(&serde_json::json!({"code":403,"data":"verify=x"})).is_err());
        assert!(
            api_build("https://challenges.cloudflare.com/turnstile/v0/b/d2d3316b729e/api.js")
                .is_ok()
        );
        assert!(api_build("https://other.example/turnstile/v0/b/d2d3316b729e/api.js").is_err());
    }

    #[test]
    fn search_form_preserves_legacy_bytes_and_never_posts_other_sites() {
        let url = "https://www.69shuba.com/modules/article/search.php?searchkey=%CC%EC%B9%D9&searchtype=all";
        assert_eq!(
            search_form(url).unwrap().unwrap(),
            b"searchkey=%CC%EC%B9%D9&searchtype=all"
        );
        assert!(
            search_form("https://other.example/modules/article/search.php?searchkey=word")
                .unwrap()
                .is_none()
        );
        assert!(search_form(
            "https://www.69shuba.com/modules/article/search.php?searchid=123&page=2"
        )
        .unwrap()
        .is_none());
        assert!(search_form(&format!("{url}&searchkey=second")).is_err());
    }
}
