//! Read-only reviews, normalized for the existing Legado review callbacks.
use crate::{
    api::{Api, number, params},
    model,
};
use anyhow::{Result, ensure};
use serde_json::{Value, json};
use std::collections::HashSet;

pub fn distinct_page(data: &mut Value, seen: &mut Vec<String>) {
    let mut ids: HashSet<String> = seen.iter().cloned().collect();
    if let Some(items) = data["items"].as_array_mut() {
        items.retain(|item| {
            let id = model::s(item, "id");
            if id.is_empty() || id.ends_with(':') {
                return true;
            }
            if !ids.insert(id.clone()) {
                return false;
            }
            seen.push(id);
            true
        });
    }
}

fn text(v: &Value, keys: &[&str]) -> String {
    model::first(v, keys).map(model::string).unwrap_or_default()
}
fn safe_image(v: &Value) -> Option<String> {
    let s = if v.is_object() {
        text(v, &["web_uri", "url", "src", "image_url"])
    } else {
        model::string(v)
    };
    let u = url::Url::parse(&s).ok()?;
    matches!(u.scheme(), "http" | "https").then_some(s)
}
fn images(v: &Value) -> Vec<String> {
    let rows = v.as_array().or_else(|| v["image_data"].as_array());
    rows.into_iter().flatten().filter_map(safe_image).collect()
}
pub fn normalize(row: &Value, chapter: bool) -> Option<Value> {
    let post = row.get("post_data");
    let c = post.or_else(|| row.get("comment")).unwrap_or(row);
    let common = c.get("common").or_else(|| c.get("Common")).unwrap_or(c);
    let user = common
        .get("user_info")
        .or_else(|| c.get("user_info"))
        .unwrap_or(&Value::Null);
    let base = user.get("base_info").unwrap_or(user);
    let content = &common["content"];
    let decoded = content
        .as_str()
        .and_then(|s| serde_json::from_str::<Value>(s).ok());
    let body = decoded.as_ref().unwrap_or(content);
    let mut message = text(body, &["text"]);
    if message.is_empty() {
        message = text(common, &["text"]);
    }
    if message.is_empty() && content.is_string() && decoded.is_none() {
        message = model::string(content);
    }
    if message.is_empty() {
        if let Some(materials) = body["materials"].as_array() {
            message = materials
                .iter()
                .filter_map(|m| model::first(m, &["text"]).map(model::string))
                .collect::<Vec<_>>()
                .join("\n");
        }
    }
    let imgs = images(&body["image_data_list"])
        .into_iter()
        .chain(images(&common["image_data"]))
        .chain(images(&body["images"]))
        .collect::<Vec<_>>();
    if message.is_empty() && imgs.is_empty() {
        return None;
    }
    let raw_id = text(c, &["comment_id", "post_id", "reply_id", "id"]);
    let prefix = if post.is_some() {
        "post"
    } else if chapter {
        "chapter"
    } else {
        "comment"
    };
    let mut badges = vec![];
    let tags = user.get("user_tag").unwrap_or(user);
    if model::boolean(&tags["is_author"]) {
        badges.push("作者".to_string());
    }
    if model::boolean(&tags["is_vip"]) {
        badges.push("VIP".to_string());
    }
    if model::boolean(&tags["is_official_cert"]) {
        badges.push("认证".to_string());
    }
    let score = text(&c["expand"], &["score"]);
    if !score.is_empty() {
        badges.push(format!("评分 {score}"));
    }
    let stat = c.get("stat").unwrap_or(c);
    let reply_to = common
        .get("reply_to_user_info")
        .or_else(|| c.get("reply_to_user_info"))
        .unwrap_or(&Value::Null);
    let reply_base = reply_to.get("base_info").unwrap_or(reply_to);
    let inline = c
        .get("reply_list")
        .or_else(|| row.get("reply_list"))
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|r| normalize(r, false))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Some(
        json!({"id":format!("{prefix}:{raw_id}"),"name":text(base,&["user_name","name"]),
        "avatar":safe_image(&base["user_avatar"]),"badge":badges,"images":imgs,
        "content":{"text":message,"img":imgs.first(),"time":model::time(common.get("create_timestamp")),
            "likeCount":number(&stat["digg_count"]),"replyCount":number(model::first(stat,&["reply_count","reply_cnt"]).unwrap_or(&Value::Null)),
            "replyToName":text(reply_base,&["user_name","name"])},"replies":inline}),
    )
}
pub fn summary(data: &Value) -> Value {
    let version = text(data, &["newest_item_version"]);
    let version = if version.is_empty() {
        "1".to_string()
    } else {
        version
    };
    let mut rows=data["idea_data"].as_object().into_iter().flatten().filter_map(|(key,v)|{
        let index=key.parse::<i32>().ok()?;let count=number(&v["idea_count"]);if index<0 || count==0{return None;}
        Some(json!({"paraIndex":if index==0 {-1}else{index},"count":count.min(i32::MAX as u64),
            "paraData":json!({"kind":"paragraph","paraIndex":index,"version":version}).to_string()}))
    }).collect::<Vec<_>>();
    rows.sort_by_key(|v| v["paraIndex"].as_i64().unwrap_or(0));
    json!(rows)
}
pub async fn get_summary(api: &mut Api, args: &Value) -> Result<Value> {
    let book = model::book_id(&args["book"])?;
    let chapter = crate::api::chapter_id(&args["chapter"])?;
    let version = text(args, &["version"]);
    let version = if version.is_empty() { "1" } else { &version };
    let raw = api
        .get(
            "/reading/ugc/idea/list/v",
            params(&[
                ("book_id", &book),
                ("item_id", &chapter),
                ("item_version", version),
            ]),
        )
        .await?;
    Ok(json!({"items":summary(&raw),"raw":raw}))
}
pub fn list_body(
    book: &str,
    group: &str,
    scope: &str,
    para: u64,
    version: &str,
    cursor: &Value,
    sort: u64,
) -> Value {
    let mut body = json!({"business_param":{"book_id":book,"fold_type":1,"item_count":0,"max_item_count":0,
        "need_count":true,"para_index":para,"read_item_count":0,"req_type":0},
        "comment_source":if scope=="book"{1}else{2},"comment_type":if scope=="book"{2}else{1},
        "count":20,"group_id":group,"group_type":if scope=="book"{1}else{15},"sort":sort,
        "cursor":if cursor.is_null(){"0".to_string()}else{model::string(cursor)}});
    if scope == "book" {
        body["server_channel"] = json!(5);
    } else {
        body["business_param"]["item_version"] = json!(version);
    }
    body
}
fn result(
    raw: Value,
    rows: &str,
    chapter: bool,
    cursor: Value,
    has_more: bool,
    page: usize,
) -> Value {
    let items = raw[rows]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| normalize(r, chapter))
        .collect::<Vec<_>>();
    json!({"items":items,"cursor":cursor,"hasMore":has_more,
        "nextPageUrl":if has_more{format!("fanqie://reviews?page={}",page+1)}else{String::new()},
        "total":raw.get("item_related_count").cloned().unwrap_or(raw["common_list_info"]["total"].clone()),"raw":raw})
}
pub async fn list(api: &mut Api, args: &Value, cursor: Value, page: usize) -> Result<Value> {
    let book = model::book_id(&args["book"])?;
    let scope = text(args, &["scope"]);
    ensure!(
        matches!(scope.as_str(), "book" | "paragraph" | "chapter"),
        "评论类型无效"
    );
    if scope == "chapter" {
        let chapter = crate::api::chapter_id(&args["chapter"])?;
        let offset = if cursor.is_object() {
            cursor
        } else {
            json!({"book_next_offset":0,"comment_next_offset":0,"post_next_offset":(page-1)*20,"self_offset":0,"topic_next_offset":0})
        };
        let raw=api.api("/reading/ugc/item/mix_data/get/v",vec![],Some(json!({"book_id":book,"item_id":chapter,
            "count":20,"forum_id":"","include_other_item_data":false,"offset":offset,"query_type":1,"should_not_impr":true,"source_type":51}))).await?;
        let next = raw["next_offset"].clone();
        let more = model::boolean(&raw["has_more"]);
        return Ok(result(raw, "mix_data", true, next, more, page));
    }
    let group = if scope == "book" {
        book.clone()
    } else {
        crate::api::chapter_id(&args["chapter"])?
    };
    let sort = args.get("sort").map(number).unwrap_or(1);
    ensure!(matches!(sort, 0 | 1 | 3), "评论排序无效");
    let version = text(args, &["version"]);
    let version = if version.is_empty() { "1" } else { &version };
    let raw = api
        .api(
            &format!("/novel/commentapi/comment/list/{group}/v1"),
            vec![],
            Some(list_body(
                &book,
                &group,
                &scope,
                number(&args["paraIndex"]),
                version,
                &cursor,
                sort,
            )),
        )
        .await?;
    let next = raw["common_list_info"]["cursor"].clone();
    let more = model::boolean(&raw["common_list_info"]["has_more"]);
    Ok(result(raw, "data_list", false, next, more, page))
}
pub async fn replies(api: &mut Api, args: &Value, cursor: Value, page: usize) -> Result<Value> {
    let book = model::book_id(&args["book"])?;
    let key = text(args, &["reviewId"]);
    let (kind, id) = key.split_once(':').unwrap_or(("comment", &key));
    ensure!(id.parse::<u64>().is_ok() && id.len() <= 20, "评论 ID 无效");
    if matches!(kind, "post" | "chapter") {
        let path = if kind == "post" {
            "/reading/ugc/postdata/comment/v"
        } else {
            "/reading/ugc/reply/item_detail/v"
        };
        let mut p = params(&[
            ("book_id", &book),
            ("forum_book_id", &book),
            ("offset", &((page - 1) * 20).to_string()),
            ("count", "20"),
        ]);
        p.push((
            if kind == "post" {
                "post_id"
            } else {
                "comment_id"
            }
            .into(),
            id.into(),
        ));
        let raw = api.get(path, p).await?;
        let rows = if kind == "post" {
            "comment"
        } else {
            "reply_list"
        };
        let more =
            model::boolean(&raw["has_more"]) || raw[rows].as_array().is_some_and(|a| a.len() == 20);
        return Ok(result(raw, rows, false, json!(page * 20), more, page));
    }
    ensure!(kind == "comment", "评论类型无效");
    let scope = text(args, &["scope"]);
    let group = if scope == "book" {
        book.clone()
    } else {
        crate::api::chapter_id(&args["chapter"])?
    };
    let raw=api.api(&format!("/novel/commentapi/reply/list/{id}/v1"),vec![],Some(json!({
        "business_param":{"book_id":book,"fold_type":1,"item_count":0,"max_item_count":0,"need_count":true,"para_index":number(&args["paraIndex"]),"read_item_count":0,"req_type":0},
        "comment_id":id,"comment_source":502,"comment_type":if scope=="book"{2}else{1},"count":20,
        "group_id":group,"group_type":if scope=="book"{1}else{15},
        "cursor":if cursor.is_null(){"0".to_string()}else{model::string(&cursor)}}))).await?;
    let next = raw["common_list_info"]["cursor"].clone();
    let more = model::boolean(&raw["common_list_info"]["has_more"]);
    Ok(result(raw, "reply_list", false, next, more, page))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn live_recommendations_deduplicate_without_caching_or_changing_cursor() {
        let mut seen = vec!["comment:123".into()];
        let mut page = json!({"items":[{"id":"comment:123","content":{"text":"repeat"}},{"id":"comment:456"},{"id":"comment:"}],"cursor":"opaque-session","raw":{"original":"complete"}});
        distinct_page(&mut page, &mut seen);
        assert_eq!(page["items"].as_array().unwrap().len(), 2);
        assert_eq!(page["items"][0]["id"], "comment:456");
        assert_eq!(page["cursor"], "opaque-session");
        assert_eq!(page["raw"]["original"], "complete");
        assert_eq!(seen.len(), 2);
    }
    #[test]
    fn summary_preserves_api_paragraphs_and_exact_opaque_keys() {
        let s = summary(
            &json!({"idea_data":{"0":{"idea_count":3},"1":{"idea_count":7},"5":{"idea_count":0}},"newest_item_version":"v2"}),
        );
        assert_eq!(s.as_array().unwrap().len(), 2);
        assert_eq!(s[0]["paraIndex"], -1);
        assert_eq!(s[1]["paraIndex"], 1);
        let p: Value = serde_json::from_str(s[0]["paraData"].as_str().unwrap()).unwrap();
        assert_eq!(p["paraIndex"], 0);
    }
    #[test]
    fn hostile_comments_stay_plain_data_and_images_are_safe() {
        let row = json!({"comment":{"comment_id":"7633328168624898840","common":{"content":{"text":"<script>alert(1)</script>","image_data_list":{"image_data":[{"web_uri":"javascript:alert(1)"},{"web_uri":"https://example.com/a.jpg"}]}},"user_info":{"base_info":{"user_name":"A"}},"create_timestamp":1602748828},"stat":{"digg_count":12,"reply_count":3}}});
        let c = normalize(&row, false).unwrap();
        assert_eq!(c["id"], "comment:7633328168624898840");
        assert_eq!(c["content"]["text"], "<script>alert(1)</script>");
        assert_eq!(c["images"].as_array().unwrap().len(), 1);
        assert_eq!(c["content"]["replyCount"], 3);
    }
    #[test]
    fn opaque_session_cursor_is_not_replaced_by_arithmetic() {
        let b = list_body(
            "123",
            "123",
            "book",
            0,
            "1",
            &json!("{\"session_id\":\"x\",\"offset\":20}"),
            1,
        );
        assert_eq!(b["cursor"], "{\"session_id\":\"x\",\"offset\":20}");
        assert_eq!(b["group_type"], 1);
    }
}
