#![recursion_limit = "256"]
mod api;
mod community;
mod crypto;
mod discovery;
mod model;
mod monitor;
mod network;
mod progress;
mod resolver;
mod runtime;
mod session;
mod shelf;
mod web;

use anyhow::{Result, anyhow, ensure};
use api::{Account, Api, Device, number, params};
use axum::{
    Router,
    extract::{ConnectInfo, DefaultBodyLimit, Json, Request, State},
    http::{HeaderMap, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

#[derive(Clone)]
struct Service {
    network: Arc<network::Network>,
    pages: Arc<Mutex<HashMap<String, PageState>>>,
    token: Option<String>,
    monitor: monitor::Monitor,
    public_url: String,
    listen: String,
    concurrency: Arc<tokio::sync::Semaphore>,
    max_concurrency: usize,
    batch_interval_ms: u64,
    sessions: session::Sessions,
    progress_locks: progress::Locks,
    page_locks: progress::Locks,
}
#[derive(Clone, Default)]
struct PageState {
    rows: Vec<Value>,
    cursor: Value,
    cell_id: String,
    show_type: String,
    sub_id: String,
    seen: Vec<String>,
    search_cursors: HashMap<usize, Value>,
    created_ms: u64,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Call {
    operation: String,
    #[serde(default)]
    args: Value,
    #[serde(default)]
    device: Device,
    #[serde(default)]
    account: Option<Account>,
    #[serde(default = "default_os")]
    os_version: String,
}
fn default_os() -> String {
    "13".into()
}
enum Payload {
    Text(String),
    Json(Value),
    Book(model::Book),
    Books(Vec<model::Book>),
    Chapters(Vec<model::Chapter>),
}
impl Payload {
    fn encode(&self) -> Result<String> {
        Ok(match self {
            Self::Text(s) => s.clone(),
            Self::Json(v) => serde_json::to_string(v)?,
            Self::Book(b) => serde_json::to_string(b)?,
            Self::Books(b) => serde_json::to_string(b)?,
            Self::Chapters(c) => serde_json::to_string(c)?,
        })
    }
}
fn array(v: &Value) -> Vec<Value> {
    v.as_array().cloned().unwrap_or_default()
}
fn page_key(api: &Api, url: &str) -> String {
    format!(
        "{}:{}:{url}",
        api.device.device_id,
        api.account
            .as_ref()
            .map(|a| format!(
                "{}:{:x}",
                a.uid,
                md5::compute(format!("{}:{}", a.session_cookie, a.tt_token))
            ))
            .unwrap_or_default()
    )
}
fn take_page(service: &Service, key: &str, page: usize) -> PageState {
    let mut pages = service.pages.lock().unwrap();
    pages.retain(|_, s| api::now().saturating_sub(s.created_ms) < 300_000);
    if page == 1 {
        pages.remove(key);
        PageState {
            created_ms: api::now(),
            ..Default::default()
        }
    } else {
        pages.get(key).cloned().unwrap_or_else(|| PageState {
            created_ms: api::now(),
            ..Default::default()
        })
    }
}
async fn hydrate(api: &mut Api, rows: &[Value]) -> Result<Vec<model::Book>> {
    if rows.is_empty() {
        return Ok(vec![]);
    }
    let ids: Vec<_> = rows
        .iter()
        .map(|r| {
            model::first(r, &["book_id_str", "book_id"])
                .map(model::string)
                .unwrap_or_default()
        })
        .collect();
    let mut seen = HashSet::new();
    let unique: Vec<_> = ids
        .iter()
        .filter(|s| seen.insert((*s).clone()))
        .cloned()
        .collect();
    let mut by_id = HashMap::new();
    for chunk in unique.chunks(200) {
        let data = api
            .get(
                "/reading/bookapi/multi-detail/v",
                params(&[
                    ("book_id", &chunk.join(",")),
                    ("book_type", "0"),
                    ("use_shelf_book_type", "1"),
                ]),
            )
            .await?;
        for (info, _) in model::book_rows(&data) {
            by_id.insert(model::s(&info, "book_id"), info);
        }
    }
    let begin = Instant::now();
    let result = rows
        .iter()
        .zip(ids)
        .filter_map(|(row, id)| {
            let info = by_id.get(&id).unwrap_or(row);
            model::first(info, &["book_name", "name"]).map(|_| model::book(info, Some(row)))
        })
        .collect();
    api.metrics.transform_us += begin.elapsed().as_micros() as u64;
    Ok(result)
}
async fn explore(
    service: &Service,
    api: &mut Api,
    url: &str,
    page: usize,
    preferences: &Value,
) -> Result<Payload> {
    if matches!(
        url,
        "fanqie://home" | "fanqie://cell/125" | "fanqie://cell/119"
    ) {
        return recommendation(service, api, url, page).await;
    }
    if url.starts_with("fanqie://category?") || url.starts_with("fanqie://ranking?") {
        return Ok(Payload::Books(
            discovery::list(api, url, preferences, page).await?,
        ));
    }
    let group = if url.starts_with("fanqie://shelf/group?") {
        Some(model::s(&discovery::parse_url(url)?, "name"))
    } else {
        None
    };
    let key = page_key(api, url);
    let mut state = take_page(service, &key, page);
    let result = if url == "fanqie://shelf" || group.is_some() || url == "fanqie://history" {
        ensure!(api.account.is_some(), "请先在书源登录界面登录番茄账号");
        if url == "fanqie://shelf" || group.is_some() {
            if page == 1 || state.rows.is_empty() {
                let data = api
                    .get(
                        "/reading/bookapi/bookshelf/list/v",
                        params(&[("server_time", "0")]),
                    )
                    .await?;
                state.rows = array(&data["book_shelf_info_all"])
                    .into_iter()
                    .filter(|r| model::s(r, "book_type") == "0")
                    .filter(|r| {
                        group
                            .as_ref()
                            .is_none_or(|g| model::s(r, "group_name") == *g)
                    })
                    .collect();
            }
        } else {
            if state.cursor.is_null() {
                state.cursor = json!({"offset":0,"timestamp":0,"done":false})
            }
            let mut seen: HashSet<_> = state
                .rows
                .iter()
                .map(|r| {
                    model::first(r, &["book_id_str", "book_id"])
                        .map(model::string)
                        .unwrap_or_default()
                })
                .collect();
            while state.rows.len() < page * 20 && !model::boolean(&state.cursor["done"]) {
                let offset = number(&state.cursor["offset"]);
                let old_timestamp = number(&state.cursor["timestamp"]);
                let data = api
                    .get(
                        "/reading/bookapi/read_history/list/v",
                        params(&[
                            ("book_type", "0"),
                            ("offset", &offset.to_string()),
                            ("limit", "20"),
                            ("last_min_read_timestamp_ms", &old_timestamp.to_string()),
                            ("full_field", "true"),
                            ("is_first_load", if offset == 0 { "true" } else { "false" }),
                            ("query_soft_deleted", "false"),
                        ]),
                    )
                    .await?;
                let rows = array(&data["data_list"]);
                let mut timestamp = number(&data["last_min_read_timestamp_ms"]);
                for r in &rows {
                    let read_time = number(&r["read_timestamp_ms"]);
                    if read_time > 0 && (timestamp == 0 || read_time < timestamp) {
                        timestamp = read_time
                    }
                    let id = model::first(r, &["book_id_str", "book_id"])
                        .map(model::string)
                        .unwrap_or_default();
                    if !id.is_empty()
                        && !seen.contains(&id)
                        && model::s(r, "book_type") == "0"
                        && !model::boolean(&r["is_delete"])
                    {
                        seen.insert(id);
                        state.rows.push(r.clone())
                    }
                }
                let next = data
                    .get("next_offset")
                    .map(number)
                    .unwrap_or(offset + rows.len() as u64);
                let done = !model::boolean(&data["has_more"]);
                ensure!(
                    done || next != offset || timestamp != old_timestamp,
                    "番茄历史接口未推进分页，请刷新后重试"
                );
                state.cursor = json!({"offset":next,"timestamp":timestamp,"done":done});
            }
        }
        let begin = ((page - 1) * 20).min(state.rows.len());
        let end = (page * 20).min(state.rows.len());
        Payload::Books(hydrate(api, &state.rows[begin..end]).await?)
    } else {
        let rank = url.starts_with("fanqie://rank/");
        let cell = url.starts_with("fanqie://cell/");
        ensure!(url == "fanqie://home" || rank || cell, "未知番茄发现入口");
        if page == 1 || state.rows.is_empty() {
            let home = api
                .get("/reading/bookapi/bookmall/homepage/v", vec![])
                .await?;
            let begin = Instant::now();
            if url == "fanqie://home" {
                state.rows = model::book_rows(&home)
                    .into_iter()
                    .map(|(info, context)| json!({"info":info,"context":context}))
                    .collect()
            } else {
                let kind = url.rsplit('/').next().unwrap_or("");
                let home = array(&home);
                let parent = home
                    .iter()
                    .find(|v| model::s(v, "show_type") == if rank { "117" } else { kind })
                    .ok_or_else(|| anyhow!("官方首页暂未提供该推荐栏目"))?;
                let view = if rank {
                    parent["cell_data"]
                        .as_array()
                        .and_then(|a| a.iter().find(|v| model::s(v, "cell_id") == kind))
                        .ok_or_else(|| anyhow!("官方首页暂未提供该推荐栏目"))?
                } else {
                    parent
                };
                state.rows = model::book_rows(view)
                    .into_iter()
                    .map(|(info, context)| json!({"info":info,"context":context}))
                    .collect();
                state.cell_id = model::s(parent, "cell_id");
                state.show_type = model::s(parent, "show_type");
                state.sub_id = if rank { kind.into() } else { "0".into() };
                state.cursor = json!({"offset":state.rows.len(),"sessionId":"","rankVersion":""});
                state.seen = state
                    .rows
                    .iter()
                    .map(|r| model::s(&r["info"], "book_id"))
                    .collect();
            }
            api.metrics.transform_us += begin.elapsed().as_micros() as u64;
        }
        let rows = if url == "fanqie://home" {
            state
                .rows
                .iter()
                .skip((page - 1) * 20)
                .take(20)
                .cloned()
                .collect()
        } else if page == 1 {
            state.rows.clone()
        } else if model::boolean(&state.cursor["done"]) {
            vec![]
        } else {
            let data = api
                .get(
                    "/reading/bookapi/bookmall/cell/change/v",
                    params(&[
                        ("cell_id", &state.cell_id),
                        ("show_type", &state.show_type),
                        ("cell_sub_id", &state.sub_id),
                        ("offset", &model::s(&state.cursor, "offset")),
                        ("limit", "10"),
                        ("change_type", "0"),
                        ("filter_ids", &state.seen.join(",")),
                        ("session_id", &model::s(&state.cursor, "sessionId")),
                        ("rank_version", &model::s(&state.cursor, "rankVersion")),
                    ]),
                )
                .await?;
            let mut view = data.get("cell_view").unwrap_or(&data);
            if rank {
                if let Some(cells) = view["cell_data"].as_array() {
                    view = cells
                        .iter()
                        .find(|v| model::s(v, "cell_id") == state.sub_id)
                        .unwrap_or(&Value::Null)
                }
            }
            let rows: Vec<_> = model::book_rows(view)
                .into_iter()
                .filter(|(info, _)| !state.seen.contains(&model::s(info, "book_id")))
                .map(|(info, context)| json!({"info":info,"context":context}))
                .collect();
            state
                .seen
                .extend(rows.iter().map(|r| model::s(&r["info"], "book_id")));
            state.cursor = json!({"offset":data.get("next_offset").map(number).unwrap_or(number(&state.cursor["offset"])+10),"sessionId":model::s(&data,"session_id"),"rankVersion":model::s(&data,"rank_version"),"done":!model::boolean(&data["has_more"])});
            rows
        };
        let begin = Instant::now();
        let books = rows
            .iter()
            .map(|r| model::book(&r["info"], r.get("context").filter(|v| !v.is_null())))
            .collect();
        api.metrics.transform_us += begin.elapsed().as_micros() as u64;
        Payload::Books(books)
    };
    state.created_ms = api::now();
    service.pages.lock().unwrap().insert(key, state);
    Ok(result)
}
/// Keep only the unconsumed first-screen rows and pagination identity. Every
/// subsequent feed/exchange request reads live data from the official API.
async fn recommendation(
    service: &Service,
    api: &mut Api,
    url: &str,
    page: usize,
) -> Result<Payload> {
    let key = page_key(api, url);
    let lock = service.page_locks.get("recommendation", &key);
    let _guard = tokio::time::timeout(Duration::from_secs(30), lock.lock())
        .await
        .map_err(|_| anyhow!("推荐加载排队超时，请稍后重试"))?;
    let mut state = take_page(service, &key, page);
    if state.cell_id.is_empty() {
        if url == "fanqie://cell/119" {
            let home = api
                .get("/reading/bookapi/bookmall/homepage/v", vec![])
                .await?;
            let parent = home
                .as_array()
                .and_then(|a| a.iter().find(|v| model::s(v, "show_type") == "119"))
                .ok_or_else(|| anyhow!("官方首页暂未提供高分栏目"))?;
            state.cell_id = model::s(parent, "cell_id");
            state.show_type = "119".into();
            state.rows = recommendation_rows(parent, &mut state.seen);
            state.cursor = json!({"offset":0});
        } else {
            let home = api
                .get(
                    "/reading/bookapi/bookmall/tab/v",
                    params(&[("tab_type", "2")]),
                )
                .await?;
            let tab = home["tab_item"]
                .as_array()
                .and_then(|a| a.iter().find(|v| number(&v["tab_type"]) == 2))
                .ok_or_else(|| anyhow!("官方暂未提供推荐频道"))?;
            let cell = tab["cell_data"]
                .as_array()
                .and_then(|a| {
                    a.iter()
                        .find(|v| matches!(model::s(v, "show_type").as_str(), "337" | "384"))
                })
                .ok_or_else(|| anyhow!("官方暂未提供连续推荐流"))?;
            state.cell_id = model::s(cell, "cell_id");
            state.show_type = model::s(cell, "show_type");
            state.cursor = json!({"offset":model::book_rows(cell).len(),"sessionId":model::s(tab,"session_id"),"done":false});
            state.rows = recommendation_rows(
                if url == "fanqie://home" { tab } else { cell },
                &mut state.seen,
            );
        }
    }
    if state.rows.is_empty() && !model::boolean(&state.cursor["done"]) {
        // EXCHANGE requests restart the high-score selection, excluding all
        // displayed books. LANDPAGE advances the real unlimited feed cursor.
        let high = url == "fanqie://cell/119";
        let recent = state
            .seen
            .iter()
            .rev()
            .take(500)
            .cloned()
            .collect::<Vec<_>>()
            .join(",");
        let data = api
            .get(
                "/reading/bookapi/bookmall/cell/change/v",
                params(&[
                    ("cell_id", &state.cell_id),
                    ("show_type", &state.show_type),
                    ("cell_sub_id", "0"),
                    ("tab_type", "2"),
                    (
                        "offset",
                        &if high {
                            "0".into()
                        } else {
                            model::s(&state.cursor, "offset")
                        },
                    ),
                    ("limit", "20"),
                    ("change_type", if high { "0" } else { "1" }),
                    ("filter_ids", &recent),
                    ("force_filter_ids", &recent),
                    ("session_id", &model::s(&state.cursor, "sessionId")),
                ]),
            )
            .await?;
        state.rows = recommendation_rows(data.get("cell_view").unwrap_or(&data), &mut state.seen);
        let next = data
            .get("next_offset")
            .map(number)
            .unwrap_or(number(&state.cursor["offset"]) + 20);
        ensure!(
            high || !model::boolean(&data["has_more"]) || next != number(&state.cursor["offset"]),
            "官方推荐游标未推进，请刷新重试"
        );
        state.cursor = json!({"offset":next,"sessionId":model::s(&data,"session_id"),"done":!high && !model::boolean(&data["has_more"])});
        ensure!(
            !state.rows.is_empty() || model::boolean(&state.cursor["done"]),
            "官方暂未返回新的推荐，请刷新或稍后重试"
        );
    }
    let begin = Instant::now();
    let count = state.rows.len().min(20);
    let books = state
        .rows
        .drain(..count)
        .map(|r| model::book(&r["info"], r.get("context").filter(|v| !v.is_null())))
        .collect();
    api.metrics.transform_us += begin.elapsed().as_micros() as u64;
    state.created_ms = api::now();
    service.pages.lock().unwrap().insert(key, state);
    Ok(Payload::Books(books))
}
fn recommendation_rows(value: &Value, seen: &mut Vec<String>) -> Vec<Value> {
    let mut known: HashSet<_> = seen.iter().cloned().collect();
    model::book_rows(value)
        .into_iter()
        .filter_map(|(info, context)| {
            let id = model::s(&info, "book_id");
            if !known.insert(id.clone()) {
                return None;
            }
            seen.push(id);
            Some(json!({"info":info,"context":context}))
        })
        .collect()
}
#[cfg(test)]
mod recommendation_tests {
    use super::*;
    #[test]
    fn feed_excludes_previous_books_and_preserves_new_row_metadata() {
        let mut seen = vec!["7000000000000000001".to_string()];
        let data = json!({"cell_data":[
            {"book_info":{"book_id":"7000000000000000001","book_name":"already shown"}},
            {"book_info":{"book_id":"7000000000000000002","book_name":"new"},"recommend_extra":"official context"},
            {"book_info":{"book_id":"7000000000000000002","book_name":"duplicate"}}
        ]});
        let rows = recommendation_rows(&data, &mut seen);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["context"]["recommend_extra"], "official context");
        assert_eq!(seen, vec!["7000000000000000001", "7000000000000000002"]);
        assert!(recommendation_rows(&data, &mut seen).is_empty());
    }
}
async fn execute(service: &Service, api: &mut Api, call: &Call) -> Result<Payload> {
    if call.operation != "transform_chapters" {
        api.ensure_device().await?
    }
    let args = &call.args;
    match call.operation.as_str() {
        "detail" => {
            let input = model::first(&args["book"], &["bookUrl", "tocUrl", "bookId"])
                .map(model::string)
                .unwrap_or_default();
            let id = resolver::resolve(api, &input).await?;
            let info = api
                .get("/reading/bookapi/detail/v", params(&[("book_id", &id)]))
                .await?;
            let start = Instant::now();
            let b = model::book(&info, model::reading(&args["book"]).as_ref());
            api.metrics.transform_us += start.elapsed().as_micros() as u64;
            Ok(Payload::Book(b))
        }
        "chapters" | "transform_chapters" => {
            let data = if call.operation == "transform_chapters" {
                let mut data = args["data"].clone();
                model::protect_ids(&mut data);
                data
            } else {
                let id = model::book_id(&args["book"])?;
                api.get(
                    "/reading/bookapi/directory/all_items/v",
                    params(&[("book_id", &id), ("filter_copyright_page", "false")]),
                )
                .await?
            };
            let start = Instant::now();
            let result = model::chapters(&data, model::reading(&args["book"]).as_ref())?;
            api.metrics.transform_us += start.elapsed().as_micros() as u64;
            Ok(Payload::Chapters(result))
        }
        "content" => Ok(Payload::Text(
            api.content(&args["book"], &args["chapter"]).await?,
        )),
        "content_batch" => {
            let mut data = api
                .content_batch(
                    &args["book"],
                    args["chapters"]
                        .as_array()
                        .ok_or_else(|| anyhow!("章节列表无效"))?,
                )
                .await?;
            if model::boolean(&args["native"]) {
                api::native_batch(&mut data);
            }
            Ok(Payload::Json(data))
        }
        "progress_get" | "progress_put" => {
            let uid = api.account.as_ref().map(|a| a.uid.as_str()).unwrap_or("");
            let lock = service
                .progress_locks
                .get(uid, &model::book_id(&args["book"])?);
            let _guard = tokio::time::timeout(Duration::from_secs(30), lock.lock())
                .await
                .map_err(|_| anyhow!("番茄章节进度同步排队超时，请稍后重试"))?;
            Ok(Payload::Json(if call.operation == "progress_get" {
                progress::get(api, &args["book"]).await?
            } else {
                progress::put(api, args).await?
            }))
        }
        "explore" => {
            explore(
                service,
                api,
                &model::s(args, "url"),
                number(&args["page"]).max(1) as usize,
                &args["preferences"],
            )
            .await
        }
        "search" => {
            let page = number(&args["page"]).max(1) as usize;
            let query = model::s(args, "key");
            if model::input_book_id(&query).is_some() || resolver::share_input(&query).is_some() {
                if page > 1 {
                    return Ok(Payload::Books(vec![]));
                }
                let id = resolver::resolve(api, &query).await?;
                let data = api
                    .get("/reading/bookapi/detail/v", params(&[("book_id", &id)]))
                    .await?;
                return Ok(Payload::Books(vec![model::book(&data, None)]));
            }
            let key = page_key(api, &format!("search:{query}"));
            let mut state = take_page(service, &key, page);
            let cursor = state
                .search_cursors
                .get(&page)
                .cloned()
                .unwrap_or(json!({"offset":(page-1)*10,"searchId":""}));
            if model::boolean(&cursor["done"]) {
                return Ok(Payload::Books(vec![]));
            }
            let data = api
                .get(
                    "/reading/bookapi/search/search/v",
                    params(&[
                        ("q", &query),
                        ("offset", &model::s(&cursor, "offset")),
                        ("search_id", &model::s(&cursor, "searchId")),
                    ]),
                )
                .await?;
            state.search_cursors.insert(page+1,json!({"offset":number(&cursor["offset"])+10,"searchId":model::s(&data,"search_id"),"done":number(&data["has_more"])!=1}));
            service.pages.lock().unwrap().insert(key, state);
            let start = Instant::now();
            let books = array(&data["search_result"])
                .iter()
                .filter(|r| model::s(r, "book_type") == "0" && model::s(r, "type") == "0")
                .map(|r| model::book(r, None))
                .collect();
            api.metrics.transform_us += start.elapsed().as_micros() as u64;
            Ok(Payload::Books(books))
        }
        "discovery_menu" => {
            let data = discovery::menu(api, &args["preferences"]).await?;
            Ok(Payload::Json(if model::boolean(&args["native"]) {
                data["entries"].clone()
            } else {
                data
            }))
        }
        "review_summary" => {
            let data = community::get_summary(api, args).await?;
            Ok(Payload::Json(if model::boolean(&args["native"]) {
                data["items"].clone()
            } else {
                data
            }))
        }
        "reviews" | "review_replies" => {
            let page = number(&args["page"]).max(1) as usize;
            let mut identity = args.clone();
            if let Some(m) = identity.as_object_mut() {
                m.remove("page");
                m.remove("cursor");
            }
            let key = page_key(api, &format!("{}:{identity}", call.operation));
            let mut state = take_page(service, &key, page);
            let cursor = args
                .get("cursor")
                .cloned()
                .or_else(|| state.search_cursors.get(&page).cloned())
                .unwrap_or(Value::Null);
            ensure!(
                page == 1 || !cursor.is_null(),
                "评论分页游标已过期，请从第一页重新加载"
            );
            let mut data = if call.operation == "reviews" {
                community::list(api, args, cursor, page).await?
            } else {
                community::replies(api, args, cursor, page).await?
            };
            community::distinct_page(&mut data, &mut state.seen);
            state
                .search_cursors
                .insert(page + 1, data["cursor"].clone());
            service.pages.lock().unwrap().insert(key, state);
            if model::boolean(&args["native"]) {
                data.as_object_mut().unwrap().remove("raw");
            }
            Ok(Payload::Json(data))
        }
        "shelf_change" => {
            let id = model::book_id(&args["book"])?;
            let uid = api.account.as_ref().map(|a| a.uid.as_str()).unwrap_or("");
            let lock = service.progress_locks.get(uid, &format!("shelf:{id}"));
            let _guard = lock.lock().await;
            Ok(Payload::Json(shelf::change(api, args).await?))
        }
        "web_login" => Ok(Payload::Json(
            json!({"login":api.web_login(&model::s(args,"cookie")).await?,"close":true}),
        )),
        "raw" => {
            let path = model::s(args, "path");
            ensure!(web::allowed_path(&path), "不支持的官方接口路径");
            let query = args["params"]
                .as_object()
                .map(|m| {
                    m.iter()
                        .map(|(k, v)| (k.clone(), model::string(v)))
                        .collect()
                })
                .unwrap_or_default();
            let body = args.get("body").filter(|v| !v.is_null()).cloned();
            ensure!(
                body.is_none() || web::allowed_post_path(&path),
                "该官方接口仅支持只读 GET"
            );
            Ok(Payload::Json(api.api(&path, query, body).await?))
        }
        _ => Err(anyhow!("Unsupported relay operation")),
    }
}
struct CallOutcome {
    operation: String,
    summary: String,
    error: Option<String>,
    detail: Arc<monitor::RequestDetail>,
}
fn authorized(service: &Service, headers: &HeaderMap) -> bool {
    service.token.as_ref().is_none_or(|token| {
        headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            == Some(&format!("Bearer {token}"))
    })
}
fn error_response(status: StatusCode, code: &str, message: &str) -> Response {
    (status, Json(json!({"code":code,"error":message}))).into_response()
}
fn validate(call: &Call) -> Option<Response> {
    let args = &call.args;
    match call.operation.as_str() {
        "detail" | "chapters" | "content" | "content_batch" | "progress_get" | "progress_put" => {
            let share_detail = call.operation == "detail"
                && resolver::share_input(&model::s(&args["book"], "bookUrl")).is_some();
            if model::book_id(&args["book"]).is_err() && !share_detail {
                return Some(error_response(
                    StatusCode::BAD_REQUEST,
                    "INVALID_BOOK",
                    "请填写有效书籍 ID 或番茄书籍地址",
                ));
            }
            if matches!(call.operation.as_str(), "content" | "progress_put")
                && api::chapter_id(&args["chapter"]).is_err()
            {
                return Some(error_response(
                    StatusCode::BAD_REQUEST,
                    "INVALID_CHAPTER",
                    "请填写有效章节 ID 或章节地址",
                ));
            }
            if call.operation == "content_batch"
                && !args["chapters"].as_array().is_some_and(|a| {
                    !a.is_empty() && a.len() <= 30 && a.iter().all(|c| api::chapter_id(c).is_ok())
                })
            {
                return Some(error_response(
                    StatusCode::BAD_REQUEST,
                    "INVALID_BATCH",
                    "请提供 1 到 30 个有效章节地址",
                ));
            }
            if call.operation.starts_with("progress_")
                && !call.account.as_ref().is_some_and(|a| {
                    !a.uid.is_empty() && (!a.session_cookie.is_empty() || !a.tt_token.is_empty())
                })
            {
                return Some(error_response(
                    StatusCode::UNAUTHORIZED,
                    "LOGIN_REQUIRED",
                    "请先在书源登录界面登录番茄账号",
                ));
            }
        }
        "explore" => {
            let url = model::s(args, "url");
            let parameters = if url.starts_with("fanqie://category?") {
                discovery::parse_url(&url).and_then(|a| {
                    discovery::category_params(&a, &args["preferences"], 1).map(|_| ())
                })
            } else if url.starts_with("fanqie://ranking?") {
                discovery::parse_url(&url).and_then(|a| discovery::rank_params(&a, 1).map(|_| ()))
            } else {
                Ok(())
            };
            if parameters.is_err() {
                return Some(error_response(
                    StatusCode::BAD_REQUEST,
                    "INVALID_FILTER",
                    "分类或榜单筛选值无效",
                ));
            }
            if !matches!(
                url.as_str(),
                "fanqie://home" | "fanqie://shelf" | "fanqie://history"
            ) && !url.starts_with("fanqie://rank/")
                && !url.starts_with("fanqie://cell/")
                && !url.starts_with("fanqie://category?")
                && !url.starts_with("fanqie://ranking?")
                && !url.starts_with("fanqie://shelf/group?")
            {
                return Some(error_response(
                    StatusCode::BAD_REQUEST,
                    "INVALID_EXPLORE",
                    "请选择有效的发现入口",
                ));
            }
            if (matches!(url.as_str(), "fanqie://shelf" | "fanqie://history")
                || url.starts_with("fanqie://shelf/group?"))
                && call.account.is_none()
            {
                return Some(error_response(
                    StatusCode::UNAUTHORIZED,
                    "LOGIN_REQUIRED",
                    "请先在阅读的书源登录界面完成官方网页登录。网页调试可在完整请求中提供 account 会话。",
                ));
            }
        }
        "search" if model::s(args, "key").trim().is_empty() => {
            return Some(error_response(
                StatusCode::BAD_REQUEST,
                "EMPTY_QUERY",
                "请输入搜索关键词",
            ));
        }
        "search" | "web_login" | "transform_chapters" => (),
        "discovery_menu" => {
            if discovery::gender(&args["preferences"]).is_err()
                || discovery::filters(&args["preferences"]).is_err()
            {
                return Some(error_response(
                    StatusCode::BAD_REQUEST,
                    "INVALID_FILTER",
                    "发现筛选值无效",
                ));
            }
        }
        "review_summary" | "reviews" | "review_replies" | "shelf_change" => {
            if model::book_id(&args["book"]).is_err() {
                return Some(error_response(
                    StatusCode::BAD_REQUEST,
                    "INVALID_BOOK",
                    "书籍地址无效",
                ));
            }
            if call.operation == "review_summary"
                || (matches!(call.operation.as_str(), "reviews" | "review_replies")
                    && model::s(args, "scope") != "book")
            {
                if api::chapter_id(&args["chapter"]).is_err() {
                    return Some(error_response(
                        StatusCode::BAD_REQUEST,
                        "INVALID_CHAPTER",
                        "章节地址无效",
                    ));
                }
            }
            if call.operation == "shelf_change" && call.account.is_none() {
                return Some(error_response(
                    StatusCode::UNAUTHORIZED,
                    "LOGIN_REQUIRED",
                    "请先登录番茄账号",
                ));
            }
            if call.operation == "shelf_change"
                && !matches!(model::s(args, "action").as_str(), "add" | "remove")
            {
                return Some(error_response(
                    StatusCode::BAD_REQUEST,
                    "INVALID_ACTION",
                    "书架操作请选择 add 或 remove",
                ));
            }
            if matches!(call.operation.as_str(), "reviews" | "review_replies")
                && !matches!(
                    model::s(args, "scope").as_str(),
                    "book" | "chapter" | "paragraph"
                )
            {
                return Some(error_response(
                    StatusCode::BAD_REQUEST,
                    "INVALID_SCOPE",
                    "评论类型无效",
                ));
            }
        }
        "raw"
            if web::allowed_path(&model::s(args, "path"))
                && (args.get("body").is_none_or(Value::is_null)
                    || web::allowed_post_path(&model::s(args, "path"))) =>
        {
            ()
        }
        _ => {
            return Some(error_response(
                StatusCode::BAD_REQUEST,
                "UNKNOWN_OPERATION",
                "未知操作，请使用接口工作台中列出的操作",
            ));
        }
    }
    if number(&args["page"]) > i32::MAX as u64 {
        return Some(error_response(
            StatusCode::BAD_REQUEST,
            "INVALID_PAGE",
            "页码超过阅读程序支持的整数范围",
        ));
    }
    None
}
async fn observe(State(service): State<Service>, request: Request, next: Next) -> Response {
    if request.uri().path() != "/v1/call" {
        return next.run(request).await;
    }
    let peer = request
        .extensions()
        .get::<ConnectInfo<monitor::Peer>>()
        .map(|p| p.0.clone())
        .unwrap_or(monitor::Peer {
            address: "127.0.0.1:0".parse().unwrap(),
            connection_id: 0,
        });
    let client = request
        .headers()
        .get("x-relay-client")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("API")
        .chars()
        .take(40)
        .collect();
    let guard = service.monitor.begin(peer, client);
    let request_id = guard.id();
    let mut response = next.run(request).await;
    response.headers_mut().insert(
        "x-fanqie-request-id",
        request_id.to_string().parse().unwrap(),
    );
    if let Some(outcome) = response.extensions().get::<Arc<CallOutcome>>() {
        guard.finish(
            response.status().as_u16(),
            outcome.operation.clone(),
            outcome.summary.clone(),
            outcome.error.clone(),
            Some(outcome.detail.clone()),
        );
    } else {
        guard.finish(
            response.status().as_u16(),
            "invalid".into(),
            "请求未进入处理流程".into(),
            Some(
                if response.status() == StatusCode::UNAUTHORIZED {
                    "服务令牌未配置或不正确"
                } else {
                    "请求格式或参数无效"
                }
                .into(),
            ),
            None,
        );
    }
    response
}
async fn call(
    State(service): State<Service>,
    ConnectInfo(peer): ConnectInfo<monitor::Peer>,
    headers: HeaderMap,
    Json(call): Json<Call>,
) -> Response {
    let started = Instant::now();
    let mut public_request = serde_json::to_value(&call).unwrap();
    monitor::redact(&mut public_request);
    let summary = web::call_summary(&call);
    let initial_error = if !authorized(&service, &headers) {
        Some(error_response(
            StatusCode::UNAUTHORIZED,
            "TOKEN_REQUIRED",
            "服务令牌未配置或不正确",
        ))
    } else {
        validate(&call)
    };
    let fallback = format!(
        "{}:{}",
        peer.address.ip(),
        headers
            .get("x-relay-client")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("API")
    );
    let session = initial_error.is_none().then(|| {
        service
            .sessions
            .get(&call.device, call.account.as_ref(), &fallback)
    });
    // Wait for this device before taking a global slot: a book download must
    // not reserve every slot and block search, discovery, and other clients.
    let content_guard = if let Some(session) = session
        .as_ref()
        .filter(|_| matches!(call.operation.as_str(), "content" | "content_batch"))
    {
        tokio::time::timeout(
            Duration::from_secs(45),
            session.content.clone().lock_owned(),
        )
        .await
        .map(Some)
    } else {
        Ok(None)
    };
    let permit = if initial_error.is_none() && content_guard.is_ok() {
        match tokio::time::timeout(Duration::from_secs(10), service.concurrency.acquire()).await {
            Ok(Ok(p)) => Some(p),
            _ => None,
        }
    } else {
        None
    };
    if let Some(mut response) = initial_error.or_else(|| {
        if content_guard.is_err() {
            Some(error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                "BUSY",
                "同设备正文请求排队超时，请降低下载并发后重试",
            ))
        } else if permit.is_none() {
            Some(error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                "BUSY",
                "服务当前繁忙，请稍后重试",
            ))
        } else {
            None
        }
    }) {
        let (parts, body) = response.into_parts();
        let bytes = axum::body::to_bytes(body, 65536).await.unwrap_or_default();
        let recorded = String::from_utf8_lossy(&bytes).into_owned();
        let error = serde_json::from_str::<Value>(&recorded)
            .ok()
            .map(|v| model::s(&v, "error"));
        response = Response::from_parts(parts, axum::body::Body::from(bytes));
        response.extensions_mut().insert(Arc::new(CallOutcome {
            operation: call.operation.clone(),
            summary,
            error,
            detail: Arc::new(monitor::RequestDetail {
                request: public_request,
                response: recorded,
                metrics: Default::default(),
                upstream: vec![],
            }),
        }));
        return response;
    }
    let Some(session) = session else {
        return error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "BUSY",
            "设备状态尚未准备完成，请重试",
        );
    };
    let snapshot = session.snapshot();
    let mut api = Api {
        client: service.network.client(),
        device: snapshot.device.clone(),
        account: call.account.clone(),
        os_version: call.os_version.clone(),
        metrics: api::Metrics {
            queue_ms: started.elapsed().as_millis() as u64,
            ..Default::default()
        },
        traces: vec![],
    };
    let result = execute(&service, &mut api, &call).await.and_then(|result| {
        let start = Instant::now();
        let body = result.encode()?;
        api.metrics.serialize_us += start.elapsed().as_micros() as u64;
        Ok((result, body))
    });
    api.device = service
        .sessions
        .publish(&session, &snapshot, api.device, call.account.as_ref());
    api.metrics.total_ms = started.elapsed().as_millis() as u64;
    let mut recorded_body: String;
    let mut error = None;
    let mut response = match result {
        Ok((kind, body)) => {
            recorded_body = body.clone();
            if call.operation == "web_login" {
                if let Ok(mut value) = serde_json::from_str::<Value>(&body) {
                    monitor::redact(&mut value);
                    recorded_body = value.to_string();
                }
            }
            let content_type = if matches!(kind, Payload::Text(_)) {
                "text/plain; charset=utf-8"
            } else {
                "application/json; charset=utf-8"
            };
            (
                [
                    (header::CONTENT_TYPE, content_type),
                    (header::CACHE_CONTROL, "no-store"),
                ],
                body,
            )
                .into_response()
        }
        Err(err) => {
            let message = web::safe_error(&err);
            error = Some(message.clone());
            recorded_body = json!({"code":"UPSTREAM_ERROR","error":message}).to_string();
            error_response(StatusCode::BAD_GATEWAY, "UPSTREAM_ERROR", &message)
        }
    };
    response.headers_mut().insert(
        "x-fanqie-state",
        STANDARD
            .encode(serde_json::to_vec(&api.device).unwrap())
            .parse()
            .unwrap(),
    );
    response.headers_mut().insert(
        "x-fanqie-metrics",
        serde_json::to_string(&api.metrics)
            .unwrap()
            .parse()
            .unwrap(),
    );
    response.extensions_mut().insert(Arc::new(CallOutcome {
        operation: call.operation.clone(),
        summary,
        error,
        detail: Arc::new(monitor::RequestDetail {
            request: public_request,
            response: recorded_body,
            metrics: api.metrics,
            upstream: api.traces,
        }),
    }));
    response
}
#[tokio::main]
async fn main() -> Result<()> {
    let listen = std::env::var("FANQIE_RELAY_LISTEN").unwrap_or_else(|_| "127.0.0.1:19670".into());
    let token = std::env::var("FANQIE_RELAY_TOKEN")
        .ok()
        .filter(|s| !s.is_empty());
    let network = Arc::new(network::Network::new()?);
    let listener = tokio::net::TcpListener::bind(&listen).await?;
    let actual_listen = listener.local_addr()?.to_string();
    let public_url = web::public_url(listener.local_addr()?.port())?;
    let max_concurrency = std::env::var("FANQIE_RELAY_CONCURRENCY")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(8)
        .clamp(1, 64);
    let service = Service {
        network,
        pages: Default::default(),
        token,
        monitor: monitor::Monitor::new(),
        public_url,
        listen: actual_listen,
        concurrency: Arc::new(tokio::sync::Semaphore::new(max_concurrency)),
        max_concurrency,
        batch_interval_ms: 0,
        sessions: Default::default(),
        progress_locks: Default::default(),
        page_locks: Default::default(),
    };
    let app=Router::new().route("/health",get(||async{Json(json!({"status":"ok","version":env!("CARGO_PKG_VERSION"),"responseCache":false}))})).route("/v1/call",post(call))
        .merge(web::routes()).layer(DefaultBodyLimit::max(8*1024*1024))
        .layer(middleware::from_fn_with_state(service.clone(),observe))
        .layer(tower_http::compression::CompressionLayer::new()).with_state(service.clone());
    println!(
        "fanqie-relay {} listening on {listen}",
        env!("CARGO_PKG_VERSION")
    );
    println!(
        "Web console: http://127.0.0.1:{} | Phone access: {}",
        listener.local_addr()?.port(),
        service.public_url
    );
    axum::serve(
        monitor::TrackedListener {
            listener,
            monitor: service.monitor,
        },
        app.into_make_service_with_connect_info::<monitor::Peer>(),
    )
    .with_graceful_shutdown(async {
        let _ = tokio::signal::ctrl_c().await;
    })
    .await?;
    Ok(())
}
