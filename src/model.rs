use anyhow::{Result, ensure};
use chrono::{DateTime, FixedOffset};
use scraper::{Html, Selector, node::Node};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::HashSet;

pub fn string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => "null".into(),
        _ => v.to_string(),
    }
}
pub fn s(v: &Value, key: &str) -> String {
    v.get(key).map(string).unwrap_or_default()
}
pub fn truth(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64() != Some(0.0),
        Value::String(s) => !s.is_empty(),
        _ => true,
    }
}
pub fn boolean(v: &Value) -> bool {
    v == true || v == 1 || v == "1" || v == "true"
}
pub fn first<'a>(v: &'a Value, keys: &[&str]) -> Option<&'a Value> {
    keys.iter().filter_map(|k| v.get(k)).find(|v| truth(v))
}
pub fn time(v: Option<&Value>) -> String {
    let Some(v) = v else { return String::new() };
    let raw = string(v);
    if v.is_null() || raw.is_empty() || raw == "0" {
        return String::new();
    }
    if let Ok(n) = raw.parse::<f64>() {
        if n.is_finite() {
            let ms = if n < 100_000_000_000.0 { n * 1000.0 } else { n };
            if let Some(date) = DateTime::from_timestamp_millis(ms as i64) {
                return date
                    .with_timezone(&FixedOffset::east_opt(28800).unwrap())
                    .format("%Y-%m-%d %H:%M")
                    .to_string();
            }
        }
    }
    let mut result = raw.replacen('T', " ", 1);
    if result.ends_with('Z') {
        result.pop();
    } else if result.len() >= 6 {
        if result.get(result.len() - 6..).is_some_and(|suffix| {
            (suffix.starts_with('+') || suffix.starts_with('-')) && suffix.as_bytes()[3] == b':'
        }) {
            result.truncate(result.len() - 6);
        }
    }
    result.chars().take(16).collect()
}
pub fn protect_ids(v: &mut Value) {
    match v {
        Value::Number(n) => {
            let text = n.to_string();
            let digits = text.strip_prefix('-').unwrap_or(&text);
            if digits.len() >= 16 && digits.bytes().all(|b| b.is_ascii_digit()) {
                *v = Value::String(text)
            }
        }
        Value::Array(a) => a.iter_mut().for_each(protect_ids),
        Value::Object(m) => m.values_mut().for_each(protect_ids),
        _ => {}
    }
}
pub fn metadata(v: &Value) -> String {
    json!({"fanqie":serde_json::to_string(v).unwrap()}).to_string()
}
pub fn reading(book: &Value) -> Option<Value> {
    let outer: Value = serde_json::from_str(book.get("variable")?.as_str()?).ok()?;
    let inner: Value = serde_json::from_str(outer.get("fanqie")?.as_str()?).ok()?;
    let v = inner.get("reading")?;
    if let Some(s) = v.as_str() {
        serde_json::from_str(s).ok()
    } else if v.is_null() {
        None
    } else {
        Some(v.clone())
    }
}
pub fn book_id(book: &Value) -> Result<String> {
    let url = first(book, &["bookUrl", "tocUrl", "bookId"])
        .map(string)
        .unwrap_or_default();
    input_book_id(&url).ok_or_else(|| anyhow::anyhow!("书籍地址无效"))
}
pub fn official_host(host: &str) -> bool {
    [
        "fanqienovel.com",
        "fqnovel.com",
        "snssdk.com",
        "changdunovel.com",
    ]
    .iter()
    .any(|d| host == *d || host.ends_with(&format!(".{d}")))
}
pub fn input_book_id(input: &str) -> Option<String> {
    fn valid(s: &str) -> Option<String> {
        (!s.is_empty()
            && s.len() <= 20
            && s.bytes().all(|b| b.is_ascii_digit())
            && s.parse::<u64>().ok().is_some_and(|n| n > 0))
        .then(|| s.to_string())
    }
    let raw = input.trim().strip_prefix("id:").unwrap_or(input.trim());
    if let Some(id) = valid(raw) {
        return Some(id);
    }
    let u = url::Url::parse(raw).ok()?;
    if !matches!(u.scheme(), "http" | "https")
        || !official_host(u.host_str()?)
        || !u.username().is_empty()
        || u.password().is_some()
    {
        return None;
    }
    if let Some(id) = u
        .path()
        .strip_prefix("/page/")
        .and_then(|s| valid(s.trim_end_matches('/')))
    {
        return Some(id);
    }
    for (k, v) in u.query_pairs() {
        if matches!(k.as_ref(), "book_id" | "bookId" | "bid" | "d") {
            if let Some(id) = valid(&v) {
                return Some(id);
            }
        }
    }
    None
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Book {
    name: String,
    book_url: String,
    author: String,
    intro: String,
    cover_url: String,
    kind: String,
    word_count: String,
    latest_chapter_title: String,
    variable: String,
}
pub fn book(info: &Value, context: Option<&Value>) -> Book {
    let mut kind = vec![];
    let mut extra = vec![];
    if let Some(tags) = first(info, &["tags", "category", "complete_category"]) {
        let tags = if let Some(tags) = tags.as_array() {
            tags.iter()
                .map(|v| {
                    if v.is_object() {
                        first(v, &["name", "tag_name"])
                            .map(string)
                            .unwrap_or_default()
                    } else {
                        string(v)
                    }
                })
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(",")
        } else {
            string(tags)
        };
        if !tags.is_empty() {
            kind.push(tags)
        }
    }
    match s(info, "creation_status").as_str() {
        "0" => kind.push("完结".into()),
        "1" => kind.push("连载".into()),
        _ => {}
    }
    if first(info, &["score"])
        .and_then(|v| string(v).parse::<f64>().ok())
        .unwrap_or(0.0)
        > 0.0
    {
        kind.push(format!("评分 {}", s(info, "score")))
    }
    if let Some(v) = first(info, &["read_cnt_text"]) {
        kind.push(string(v))
    }
    let updated = time(first(
        info,
        &[
            "last_publish_time",
            "last_chapter_update_time",
            "update_time",
        ],
    ));
    if !updated.is_empty() {
        extra.push(format!("更新：{updated}"))
    }
    if let Some(v) = first(info, &["copyright_info"]) {
        extra.push(string(v))
    }
    for (label, keys) in [
        ("原书名", vec!["original_book_name", "original_name"]),
        (
            "别名",
            vec![
                "book_alias",
                "alias",
                "alias_name",
                "book_flight_alias_name",
            ],
        ),
        ("短名", vec!["book_short_name"]),
        ("主角", vec!["roles", "role"]),
        ("在读人数", vec!["read_count", "reading_count"]),
    ] {
        if let Some(v) = first(info, &keys) {
            let value = string(v);
            if matches!(label, "原书名" | "别名" | "短名")
                && (value == s(info, "book_name")
                    || (label == "别名" && value == s(info, "original_book_name")))
            {
                continue;
            }
            extra.push(format!("{label}：{value}"));
        }
    }
    if let Some(c) = context {
        if let Some(v) = first(
            c,
            &[
                "item_title",
                "chapter_title",
                "last_read_item_title",
                "last_item_title",
            ],
        ) {
            extra.push(format!("阅读到：{}", string(v)))
        }
        let t = time(first(
            c,
            &[
                "last_read_timestamp_ms",
                "read_timestamp_ms",
                "last_read_time",
                "last_read_timestamp",
            ],
        ));
        if !t.is_empty() {
            extra.push(format!("最近阅读：{t}"))
        }
        if let Some(v) = c.get("chapter_index") {
            extra.push(format!(
                "阅读进度：第 {} 章",
                string(v).parse::<i64>().unwrap_or(0) + 1
            ))
        }
        if let Some(v) = first(c, &["add_shelf_time"]) {
            extra.push(format!("加入书架：{}", time(Some(v))))
        }
        if let Some(v) = first(c, &["group_name"]) {
            kind.push(format!("书架分组：{}", string(v)))
        }
        if boolean(&c["is_pin"]) {
            kind.push("置顶".into())
        }
    }
    let mut intro = vec![
        first(info, &["abstract", "book_abstract_v2", "intro"])
            .map(string)
            .unwrap_or_default(),
    ];
    intro.extend(extra);
    Book {
        name: first(info, &["book_name", "name"])
            .map(string)
            .unwrap_or_default(),
        book_url: format!("https://fanqienovel.com/page/{}", s(info, "book_id")),
        author: first(info, &["author", "author_name"])
            .map(string)
            .unwrap_or_default(),
        intro: intro
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n"),
        cover_url: first(info, &["thumb_url", "detail_page_thumb_url"])
            .map(string)
            .unwrap_or_default(),
        kind: kind.join(" | "),
        word_count: first(info, &["word_number"])
            .map(string)
            .unwrap_or_else(|| "0".into()),
        latest_chapter_title: first(info, &["last_chapter_title", "last_item_title"])
            .map(string)
            .unwrap_or_default(),
        variable: metadata(
            &context
                .map(|c| json!({"book":info,"reading":c}))
                .unwrap_or_else(|| info.clone()),
        ),
    }
}
pub fn search_book(info: &Value, cell: &Value, single_book: bool) -> Book {
    let mut item = book(info, None);
    // SearchBookCardProvider uses the official title highlight as its display
    // text. It can name the matching original title or short name plus alias.
    // A collection card's heading belongs to the collection, not each book.
    if single_book {
        if let Some(title) = first(&cell["search_high_light"]["title"], &["text"])
            .or_else(|| first(cell, &["cell_name"]))
        {
            item.name = string(title);
        }
    }
    item.variable = metadata(&json!({"book":info,"search":cell}));
    item
}
pub fn book_rows(data: &Value) -> Vec<(Value, Option<Value>)> {
    fn visit(v: &Value, seen: &mut HashSet<String>, out: &mut Vec<(Value, Option<Value>)>) {
        if let Some(a) = v.as_array() {
            for v in a {
                visit(v, seen, out)
            }
            return;
        }
        let Some(object) = v.as_object() else { return };
        let info = first(v, &["book_info", "book_data", "book"]).unwrap_or(v);
        if truth(&info["book_id"]) && first(info, &["book_name", "name"]).is_some() {
            let id = s(info, "book_id");
            if (!truth(&info["book_type"]) || s(info, "book_type") == "0")
                && (!truth(&info["type"]) || s(info, "type") == "0")
                && seen.insert(id)
            {
                out.push((
                    info.clone(),
                    if std::ptr::eq(info, v) {
                        None
                    } else {
                        Some(v.clone())
                    },
                ))
            }
            return;
        }
        for v in object.values() {
            visit(v, seen, out)
        }
    }
    let mut out = vec![];
    visit(data, &mut HashSet::new(), &mut out);
    out
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Chapter {
    title: Value,
    url: String,
    word_count: Option<String>,
    tag: Option<String>,
    is_vip: bool,
    is_pay: bool,
    variable: String,
}
pub fn chapters(data: &Value, context: Option<&Value>) -> Result<Vec<Chapter>> {
    let rows = data["item_data_list"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("番茄接口返回空目录"))?;
    ensure!(!rows.is_empty(), "番茄接口返回空目录");
    Ok(rows
        .iter()
        .enumerate()
        .map(|(index, c)| {
            let updated = time(c.get("modify_time"));
            let published = if updated.is_empty() {
                time(c.get("first_pass_time"))
            } else {
                String::new()
            };
            let mut tag = if !updated.is_empty() {
                format!("更新 {updated}")
            } else if !published.is_empty() {
                format!("发布 {published}")
            } else {
                String::new()
            };
            if let Some(volume) = first(c, &["volume_name", "volume_title"]) {
                if !tag.is_empty() {
                    tag.push_str(" · ");
                }
                tag.push_str(&string(volume));
            }
            if let Some(reading) = context {
                let id = first(reading, &["item_id", "last_item_id"])
                    .map(string)
                    .unwrap_or_default();
                let chapter_index = reading
                    .get("chapter_index")
                    .filter(|v| !v.is_null())
                    .and_then(|v| string(v).parse::<usize>().ok());
                if id == s(c, "item_id") || chapter_index == Some(index) {
                    if !tag.is_empty() {
                        tag.push_str(" · ")
                    }
                    tag.push_str("上次读至")
                }
            }
            Chapter {
                title: c["title"].clone(),
                url: format!("https://fanqienovel.com/reader/{}", s(c, "item_id")),
                word_count: c.get("chapter_word_number").map(string),
                tag: if tag.is_empty() { None } else { Some(tag) },
                is_vip: boolean(&c["show_vip_tag"]),
                is_pay: boolean(first(c, &["is_paid", "is_purchased"]).unwrap_or(&Value::Null)),
                variable: metadata(c),
            }
        })
        .collect())
}
fn element_text(element: scraper::ElementRef<'_>) -> String {
    let raw = element
        .descendants()
        .filter_map(|n| match n.value() {
            Node::Element(element) if element.name() == "br" => Some(" "),
            Node::Text(text) => {
                if n.ancestors().any(|a| {
                    a.value()
                        .as_element()
                        .is_some_and(|e| matches!(e.name(), "script" | "style"))
                }) {
                    None
                } else {
                    Some(text.text.as_ref())
                }
            }
            _ => None,
        })
        .collect::<String>();
    raw.split_whitespace().collect::<Vec<_>>().join(" ")
}
fn image_markup(element: scraper::ElementRef<'_>) -> Option<String> {
    let src = element
        .value()
        .attr("src")
        .or_else(|| element.value().attr("data-src"))?;
    let url = url::Url::parse(src).ok()?;
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    let src = url
        .as_str()
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    Some(format!("<img src=\"{src}\">"))
}
fn paragraph_content(element: scraper::ElementRef<'_>) -> String {
    let raw = element
        .descendants()
        .filter_map(|n| {
            if n.ancestors().any(|a| {
                a.value()
                    .as_element()
                    .is_some_and(|e| matches!(e.name(), "script" | "style"))
            }) {
                return None;
            }
            match n.value() {
                Node::Text(t) => Some(t.text.to_string()),
                Node::Element(e) if e.name() == "br" => Some(" ".into()),
                Node::Element(e) if e.name() == "img" => {
                    scraper::ElementRef::wrap(n).and_then(image_markup)
                }
                _ => None,
            }
        })
        .collect::<String>();
    raw.split_whitespace().collect::<Vec<_>>().join(" ")
}
pub fn content(html: &str, expected: usize) -> Result<String> {
    let doc = Html::parse_document(html);
    let p = Selector::parse("body p").unwrap();
    let headings =
        Selector::parse("body h1,body h2,body h3,body h4,body h5,body h6,body header").unwrap();
    let nested = Selector::parse("p,h1,h2,h3,h4,h5,h6").unwrap();
    let paragraphs: Vec<_> = doc.select(&p).collect();
    let mut blocks = paragraphs.len();
    for h in doc.select(&headings) {
        if h.value().name() == "header" && h.select(&nested).next().is_some() {
            continue;
        }
        if h.value().name() == "header" || !element_text(h).is_empty() {
            blocks += 1
        }
    }
    ensure!(
        expected == 0 || blocks == 0 || blocks >= expected,
        "番茄接口返回的正文段落不完整"
    );
    let lines: Vec<_> = doc
        .select(&Selector::parse("body p,body img,body aside").unwrap())
        .filter(|e| {
            !e.ancestors().any(|a| {
                a.value()
                    .as_element()
                    .is_some_and(|n| matches!(n.name(), "p" | "aside"))
            })
        })
        .map(|e| match e.value().name() {
            "img" => image_markup(e).unwrap_or_default(),
            "aside" => format!("注释：{}", element_text(e)),
            _ => paragraph_content(e),
        })
        .filter(|s| !s.is_empty())
        .collect();
    let text = if lines.is_empty() {
        doc.select(&Selector::parse("body").unwrap())
            .next()
            .map(element_text)
            .unwrap_or_default()
    } else {
        lines.join("\n\n")
    };
    ensure!(!text.is_empty(), "番茄章节正文为空");
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ids_remain_exact() {
        let mut v: Value = serde_json::from_str(
            "{\"id\":6883749008234250760,\"time\":1791302400000,\"data\":[-1234567890123456]}",
        )
        .unwrap();
        protect_ids(&mut v);
        assert_eq!(v["id"], "6883749008234250760");
        assert_eq!(v["time"], 1791302400000_u64);
        assert_eq!(v["data"][0], "-1234567890123456");
    }
    #[test]
    fn dates_and_reading_marker() {
        assert_eq!(time(Some(&json!(1602748828))), "2020-10-15 16:00");
        assert_eq!(time(Some(&json!(1791314757000_i64))), "2026-10-07 03:25");
        let data = json!({"item_data_list":[{"title":"a","item_id":"123","first_pass_time":1602748828,"chapter_word_number":42}]});
        let c = chapters(&data, Some(&json!({"chapter_index":0}))).unwrap();
        assert_eq!(
            c[0].tag.as_deref(),
            Some("发布 2020-10-15 16:00 · 上次读至")
        );
    }
    #[test]
    fn images_and_notes_survive_without_executable_html() {
        let c=content("<p>正文<a href='#note_ref_1'>[1]</a><img src='https://example.com/a.jpg?a=1&amp;b=2' onerror='alert(1)'></p><aside id='note_ref_1'><p>释义</p></aside><img src='javascript:alert(1)'><img src='https://example.com/b.jpg'>",0).unwrap();
        assert!(c.contains("正文[1]<img src=\"https://example.com/a.jpg?a=1&amp;b=2\">"));
        assert!(c.contains("注释：释义"));
        assert_eq!(c.matches("释义").count(), 1);
        assert_eq!(c.matches("/a.jpg").count(), 1);
        assert!(c.contains("/b.jpg"));
        assert!(!c.contains("onerror"));
        assert!(!c.contains("javascript"));
    }
    #[test]
    fn html_and_truncation() {
        assert_eq!(
            content(
                "<header>title</header><p>Hello <b>world</b>!</p><p>中文 &amp; 😀</p>",
                3
            )
            .unwrap(),
            "Hello world!\n\n中文 & 😀"
        );
        assert!(content("<p>only</p>", 2).is_err());
        assert_eq!(
            content("<p>a<br>b<script>ignored</script>c</p>", 1).unwrap(),
            "a bc"
        );
    }
}
