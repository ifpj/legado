//! Live category navigation and filtering. No upstream response is retained.
use crate::{
    api::{Api, number, params},
    model,
};
use anyhow::{Result, ensure};
use serde_json::{Value, json};
use std::collections::HashSet;

pub const WORDS: &[(&str, &str)] = &[
    ("不限", "word_num_default"),
    ("10万以内", "word_num_lte10"),
    ("30万以内", "word_num_lte30"),
    ("50万以内", "word_num_lte50"),
    ("30万以上", "word_num_gte30"),
    ("50万以上", "word_num_gte50"),
    ("100万以上", "word_num_gte100"),
    ("500万以上", "word_num_gte500"),
];
pub const STATUS: &[(&str, &str)] = &[
    ("不限", "creation_status_default"),
    ("完结", "creation_status_end"),
    ("连载", "creation_status_loading"),
    ("半年内完结", "creation_status_half_year_end"),
    ("3日内更新", "creation_status_3day_update"),
    ("7日内更新", "creation_status_7day_update"),
    ("1月内更新", "creation_status_1month_update"),
];
pub const SORT: &[(&str, &str)] = &[
    ("推荐", "sort_default"),
    ("最新", "sort_new_book"),
    ("高分", "sort_score"),
];
pub fn enum_value<'a>(v: &'a Value, key: &str, allowed: &[(&str, &str)]) -> Result<String> {
    let raw = model::s(v, key);
    if raw.is_empty() {
        return Ok(allowed[0].1.into());
    }
    ensure!(
        allowed.iter().any(|(_, id)| *id == raw),
        "发现筛选值无效：{key}"
    );
    Ok(raw)
}
pub fn gender(v: &Value) -> Result<String> {
    let g = v
        .get("gender")
        .map(model::string)
        .unwrap_or_else(|| "1".into());
    ensure!(matches!(g.as_str(), "0" | "1"), "发现分类请选择男生或女生");
    Ok(g)
}
pub fn filters(v: &Value) -> Result<String> {
    Ok([
        enum_value(v, "words", WORDS)?,
        enum_value(v, "status", STATUS)?,
        enum_value(v, "sort", SORT)?,
    ]
    .join(","))
}
pub fn parse_url(url: &str) -> Result<Value> {
    let u = url::Url::parse(url)?;
    ensure!(
        u.scheme() == "fanqie" && u.username().is_empty() && u.password().is_none(),
        "发现地址无效"
    );
    let mut args = json!({});
    for (k, v) in u.query_pairs() {
        args[k.as_ref()] = json!(v);
    }
    Ok(args)
}
pub fn category_params(
    args: &Value,
    preferences: &Value,
    page: usize,
) -> Result<Vec<(String, String)>> {
    let g = gender(args)?;
    let id = model::s(args, "id");
    ensure!(id.parse::<u64>().is_ok(), "分类 ID 无效");
    let limit = if id == "0" { 10 } else { 20 };
    let mut selected = filters(preferences)?;
    if let Some(sub) = args.get("sub").map(model::string).filter(|s| s != "0") {
        ensure!(sub.parse::<u64>().is_ok(), "分类标签无效");
        selected.push_str(&format!(",cate_{sub}"));
    }
    let mut query = params(&[
        ("gender", &g),
        ("query_gender", &g),
        ("genre_type", "0"),
        ("category_id", &id),
        ("selected_items", &selected),
        ("source", "front_category"),
        ("page_version", "2"),
        ("limit", &limit.to_string()),
        ("offset", &((page - 1) * limit).to_string()),
    ]);
    if id == "0" {
        // The official merged category page accepts an unrestricted category.
        // The older per-category page rejects category_id=0 (PARAM_INVALID).
        let selected = [
            enum_value(preferences, "words", WORDS)?,
            enum_value(preferences, "status", STATUS)?,
        ]
        .into_iter()
        .filter(|value| !value.ends_with("_default"))
        .collect::<Vec<_>>()
        .join(",");
        let order = match enum_value(preferences, "sort", SORT)?.as_str() {
            "sort_new_book" => "new_sort_newest",
            "sort_score" => "new_sort_score",
            _ => "new_sort_hot",
        };
        query.retain(|(key, _)| key != "selected_items");
        query.extend(params(&[
            ("category_new_page_715", "1"),
            ("is_merged_landing_page", "true"),
            ("category_type", if g == "1" { "7" } else { "8" }),
            ("client_req_type", if page == 1 { "3" } else { "2" }),
            ("no_need_all_tag", "false"),
            ("selected_item_from_front_page", "0"),
            ("selected_order", order),
        ]));
        if !selected.is_empty() {
            query.push(("selected_items".into(), selected));
        }
    }
    Ok(query)
}
pub fn rank_params(args: &Value, offset: u64) -> Result<Vec<(String, String)>> {
    let g = gender(args)?;
    let algo = model::s(args, "algo");
    ensure!(positive_id(&algo), "榜单类型无效");
    let mut p = params(&[
        ("gender", &g),
        ("gender_list_type", &g),
        ("algo_type", &algo),
        ("change_type", "1"),
        ("tab_type", "2"),
        ("limit", "20"),
        ("offset", &offset.to_string()),
    ]);
    let cell = model::s(args, "cell");
    if !cell.is_empty() {
        ensure!(positive_id(&cell), "榜单栏目 ID 无效");
        p.push(("cell_id".into(), cell));
    }
    if model::boolean(&args["strategy"]) {
        p.push(("support_gender_list".into(), "true".into()));
        p.push((
            "specific_sub_tab_type".into(),
            (g.parse::<u32>()? + 4).to_string(),
        ));
    }
    Ok(p)
}
fn positive_id(id: &str) -> bool {
    !id.is_empty()
        && id.bytes().all(|b| b.is_ascii_digit())
        && id.parse::<u64>().is_ok_and(|n| n > 0)
}
pub fn rank_menu(data: &Value, g: &str) -> Result<Vec<Value>> {
    let cell = data["tab_item"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|tab| number(&tab["tab_type"]) == 2)
        .flat_map(|tab| tab["cell_data"].as_array().into_iter().flatten())
        .find(|cell| cell["rank_with_category_data"]["rank_algo_list"].is_array())
        .ok_or_else(|| anyhow::anyhow!("官方暂未提供榜单导航，请刷新重试"))?;
    let cell_id = model::s(cell, "cell_id");
    ensure!(positive_id(&cell_id), "官方榜单栏目 ID 无效");
    let preview = cell["rank_with_category_data"]["rank_algo_list"]
        .as_array()
        .unwrap();
    // The preview contains mixed-media shortcuts. The nested landing-page URL
    // supplies the complete navigation for the novel tab, in official order.
    let mut url = model::s(cell, "cell_url");
    let mut ranks = None;
    for _ in 0..4 {
        let Ok(parsed) = url::Url::parse(&url) else {
            break;
        };
        let query: std::collections::HashMap<_, _> = parsed.query_pairs().into_owned().collect();
        if let (Some(ids), Some(names)) = (query.get("main_algo_type"), query.get("main_algo_name"))
        {
            let ids: Vec<_> = ids.split(',').collect();
            let names: Vec<_> = names.split(',').collect();
            ensure!(
                !ids.is_empty() && ids.len() == names.len(),
                "官方榜单导航名称与编号不匹配"
            );
            ranks = Some(
                ids.into_iter()
                    .zip(names)
                    .map(|(id, name)| (id.to_owned(), name.to_owned()))
                    .collect::<Vec<_>>(),
            );
            break;
        }
        url = query
            .get("url")
            .or_else(|| query.get("surl"))
            .cloned()
            .unwrap_or_default();
    }
    let ranks = ranks.unwrap_or_else(|| {
        preview
            .iter()
            .map(|rank| (model::s(rank, "rank_algo"), model::s(rank, "rank_name")))
            .collect()
    });
    let mut seen = HashSet::new();
    let mut entries = Vec::new();
    for (id, name) in ranks {
        ensure!(
            positive_id(&id) && !name.trim().is_empty(),
            "官方榜单导航名称或编号无效"
        );
        if !seen.insert(id.clone()) {
            continue;
        }
        // APK BookAlbumAlgoType: AuthorRankList, RankListBookHungerTopic,
        // short-series and animation-drama rankings return non-book entities.
        // Their official metadata remains exposed in the menu's ranking data.
        if matches!(id.as_str(), "205" | "208" | "502" | "550") {
            continue;
        }
        let strategy = preview.iter().any(|rank| {
            model::s(rank, "rank_algo") == id && model::boolean(&rank["is_strategy_rank_list"])
        });
        entries.push(item(
            &name,
            &format!("fanqie://ranking?gender={g}&algo={id}&cell={cell_id}&strategy={strategy}"),
            false,
        ));
    }
    ensure!(!entries.is_empty(), "官方榜单导航为空，请刷新重试");
    Ok(entries)
}
async fn rank_navigation(api: &mut Api, g: &str) -> Result<Value> {
    api.get(
        "/reading/bookapi/bookmall/tab/v",
        params(&[
            ("tab_type", "2"),
            ("client_fetch_unlimited_mode", "1"),
            ("cold_start_gd", g),
            ("cold_start_is_double_gd", "false"),
        ]),
    )
    .await
}
fn item(title: &str, url: &str, heading: bool) -> Value {
    json!({"title":title,"url":url,"style":{"layout_flexGrow":1,"layout_flexBasisPercent":if heading {1.0}else{0.23}}})
}
fn update_source_item() -> Value {
    let mut entry = item("更新书源", "", false);
    entry["type"] = json!("button");
    entry["action"] = json!(include_str!("../source/update-source.js"));
    entry
}
pub fn category_menu(data: &Value, g: &str) -> Vec<Value> {
    let mut menu = vec![item(
        "全部小说",
        &format!("fanqie://category?gender={g}&id=0"),
        false,
    )];
    let mut seen = HashSet::new();
    for cell in data["category_tab_data"]["cell_data"]
        .as_array()
        .into_iter()
        .flatten()
    {
        let rows = cell["atom_data"].as_array().cloned().unwrap_or_default();
        if rows.is_empty() {
            continue;
        }
        menu.push(item(&model::s(cell, "cell_name"), "", true));
        for atom in rows {
            let c = &atom["category_data"];
            let id = model::s(c, "category_id");
            let title = model::s(c, "name");
            if id.parse::<u64>().is_ok() && !title.is_empty() && seen.insert(id.clone()) {
                menu.push(item(
                    &title,
                    &format!("fanqie://category?gender={g}&id={id}"),
                    false,
                ));
            }
        }
    }
    menu
}
pub async fn menu(api: &mut Api, preferences: &Value) -> Result<Value> {
    let g = gender(preferences)?;
    let mut entries = vec![
        item("首页推荐", "fanqie://home", false),
        item("猜你喜欢", "fanqie://cell/125", false),
        item("高分佳作", "fanqie://cell/119", false),
    ];
    if api.account.is_some() {
        entries.extend([
            item("我的书架", "fanqie://shelf", false),
            item("阅读历史", "fanqie://history", false),
        ]);
    }
    entries.push(update_source_item());
    if api.account.is_some() {
        let shelf = api
            .get(
                "/reading/bookapi/bookshelf/list/v",
                params(&[("server_time", "0")]),
            )
            .await?;
        let mut groups = HashSet::new();
        for row in shelf["book_shelf_info_all"]
            .as_array()
            .into_iter()
            .flatten()
        {
            if model::s(row, "book_type") != "0" {
                continue;
            }
            let group = model::first(row, &["group_name"])
                .map(model::string)
                .unwrap_or_default();
            if groups.insert(group.clone()) {
                let title = if group.is_empty() {
                    "未分组"
                } else {
                    &group
                };
                let encoded =
                    url::form_urlencoded::byte_serialize(group.as_bytes()).collect::<String>();
                entries.push(item(
                    &format!("书架 · {title}"),
                    &format!("fanqie://shelf/group?name={encoded}"),
                    false,
                ));
            }
        }
    }
    entries.push(item(
        if g == "1" {
            "男生榜单"
        } else {
            "女生榜单"
        },
        "",
        true,
    ));
    let ranking = rank_navigation(api, &g).await?;
    entries.extend(rank_menu(&ranking, &g)?);
    let data = api
        .get(
            "/reading/bookapi/new_category/front/v",
            params(&[("distinct_style", "1"), ("new_category_tab", &g)]),
        )
        .await?;
    entries.push(item(
        if g == "1" {
            "男生分类与标签"
        } else {
            "女生分类与标签"
        },
        "",
        true,
    ));
    entries.extend(category_menu(&data, &g));
    Ok(
        json!({"entries":entries,"filters":{"words":WORDS,"status":STATUS,"sort":SORT},"raw":data,"ranking":ranking}),
    )
}
pub async fn list(
    api: &mut Api,
    url: &str,
    preferences: &Value,
    page: usize,
) -> Result<Vec<model::Book>> {
    let args = parse_url(url)?;
    let data = api
        .get(
            "/reading/bookapi/new_category/landing/v",
            category_params(&args, preferences, page)?,
        )
        .await?;
    Ok(model::book_rows(&data)
        .iter()
        .map(|(v, c)| model::book(v, c.as_ref()))
        .collect())
}
pub async fn rank_data(api: &mut Api, url: &str, cursor: &Value) -> Result<Value> {
    let mut args = parse_url(url)?;
    // Resolve older imported menu URLs against the current navigation.
    if model::s(&args, "cell").is_empty() {
        let g = gender(&args)?;
        let navigation = rank_navigation(api, &g).await?;
        let algo = model::s(&args, "algo");
        let mut current = None;
        for entry in rank_menu(&navigation, &g)? {
            let resolved = parse_url(&model::s(&entry, "url"))?;
            if model::s(&resolved, "algo") == algo {
                current = Some(resolved);
                break;
            }
        }
        args = current.ok_or_else(|| anyhow::anyhow!("官方当前没有此榜单，请刷新发现入口"))?;
    }
    let mut query = rank_params(&args, number(&cursor["offset"]))?;
    for (param, field) in [("session_id", "sessionId"), ("rank_version", "rankVersion")] {
        let value = model::s(cursor, field);
        if !value.is_empty() {
            query.push((param.into(), value));
        }
    }
    api.get("/reading/bookapi/bookmall/cell/change/v1", query)
        .await
}
pub fn rank_cursor(data: &Value, previous: &Value) -> Result<Value> {
    let done = !model::boolean(&data["has_more"]);
    let offset = number(&data["next_offset"]);
    ensure!(
        done || offset > number(&previous["offset"]),
        "官方榜单游标未推进，请刷新重试"
    );
    Ok(
        json!({"offset":offset,"sessionId":model::s(data,"session_id"),
        "rankVersion":model::s(data,"rank_version"),"done":done}),
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    fn navigation(cell: Value) -> Value {
        json!({"tab_item":[{"tab_type":2,"cell_data":[cell]}]})
    }
    #[test]
    fn complete_official_navigation_keeps_names_order_and_dynamic_ids() {
        let mut landing = url::Url::parse("https://official.example/template.js").unwrap();
        landing.query_pairs_mut().extend_pairs([
            ("main_algo_type", "108,9999,205,208,200,108"),
            (
                "main_algo_name",
                "新书榜,官方新增榜,作者榜,书荒榜,巅峰榜,重复入口",
            ),
        ]);
        let mut inner = url::Url::parse("sslocal://lynxview").unwrap();
        inner
            .query_pairs_mut()
            .append_pair("surl", landing.as_str());
        let mut outer = url::Url::parse("dragon1967://lynxview").unwrap();
        outer.query_pairs_mut().append_pair("url", inner.as_str());
        let data = navigation(json!({
            "cell_id":"9000000000000000001","cell_url":outer.as_str(),
            "rank_with_category_data":{"rank_algo_list":[
                {"rank_algo":200,"rank_name":"巅峰榜","is_strategy_rank_list":true},
                {"rank_algo":550,"rank_name":"漫剧榜"}
            ]}
        }));
        let menu = rank_menu(&data, "0").unwrap();
        assert_eq!(
            menu.iter()
                .map(|e| model::s(e, "title"))
                .collect::<Vec<_>>(),
            ["新书榜", "官方新增榜", "巅峰榜"]
        );
        let new = parse_url(&model::s(&menu[0], "url")).unwrap();
        assert_eq!(new["algo"], "108");
        let params = rank_params(&parse_url(&model::s(&menu[2], "url")).unwrap(), 40).unwrap();
        for pair in [
            ("cell_id", "9000000000000000001"),
            ("algo_type", "200"),
            ("support_gender_list", "true"),
            ("specific_sub_tab_type", "4"),
            ("offset", "40"),
        ] {
            assert!(params.contains(&(pair.0.into(), pair.1.into())));
        }
        assert!(rank_params(&parse_url(&model::s(&menu[1], "url")).unwrap(), 1).is_ok());
    }
    #[test]
    fn official_metadata_is_used_when_landing_navigation_is_absent() {
        let data = navigation(json!({"cell_id":123,"rank_with_category_data":{
            "rank_algo_list":[{"rank_algo":4321,"rank_name":"官方变更名称"}]
        }}));
        let menu = rank_menu(&data, "1").unwrap();
        assert_eq!(menu[0]["title"], "官方变更名称");
        assert_eq!(
            parse_url(&model::s(&menu[0], "url")).unwrap()["algo"],
            "4321"
        );
    }
    #[test]
    fn invalid_official_navigation_is_not_replaced_with_guessed_rankings() {
        let malformed = navigation(json!({"cell_id":123,
            "cell_url":"https://official.example/?main_algo_type=108,200&main_algo_name=only-one",
            "rank_with_category_data":{"rank_algo_list":[]}}));
        assert!(rank_menu(&malformed, "1").is_err());
        assert!(
            rank_menu(
                &navigation(json!({"cell_id":123,
            "rank_with_category_data":{"rank_algo_list":[]}})),
                "1"
            )
            .is_err()
        );
    }
    #[test]
    fn ranking_uses_official_offsets_and_stops_at_the_actual_end() {
        let first = rank_cursor(
            &json!({"has_more":true,"next_offset":40,
            "session_id":"first","rank_version":"v1"}),
            &json!({}),
        )
        .unwrap();
        assert_eq!(first["offset"], 40);
        assert_eq!(first["rankVersion"], "v1");
        let next = rank_cursor(
            &json!({"has_more":true,"next_offset":80,
            "session_id":"next","rank_version":"v1"}),
            &first,
        )
        .unwrap();
        assert_eq!(next["offset"], 80);
        assert_eq!(next["sessionId"], "next");
        assert!(rank_cursor(&json!({"has_more":true,"next_offset":40}), &first).is_err());
        assert_eq!(
            rank_cursor(&json!({"has_more":false,"next_offset":0}), &next).unwrap()["done"],
            true
        );
    }
    #[test]
    fn filters_and_urls_are_structured_not_query_injection() {
        let a = json!({"id":"7","gender":"1"});
        let p = category_params(
            &a,
            &json!({"words":"word_num_gte100","status":"creation_status_end","sort":"sort_score"}),
            2,
        )
        .unwrap();
        assert!(p.contains(&("offset".into(), "20".into())));
        assert!(p.contains(&(
            "selected_items".into(),
            "word_num_gte100,creation_status_end,sort_score".into()
        )));
        assert!(category_params(&a, &json!({"sort":"sort_score&aid=1"}), 1).is_err());
        assert!(rank_params(&json!({"gender":1,"algo":"108&aid=1"}), 1).is_err());
        assert!(rank_params(&json!({"gender":1,"algo":"108","cell":"0"}), 1).is_err());
    }
    #[test]
    fn all_novels_use_merged_page_channel_and_separate_sort() {
        for (gender, channel) in [("1", "7"), ("0", "8")] {
            let query = category_params(&json!({"id":"0","gender":gender}), &json!({}), 1).unwrap();
            assert!(query.contains(&("category_new_page_715".into(), "1".into())));
            assert!(query.contains(&("is_merged_landing_page".into(), "true".into())));
            assert!(query.contains(&("category_type".into(), channel.into())));
            assert!(!query.iter().any(|(key, _)| key == "selected_items"));
            assert!(query.contains(&("selected_order".into(), "new_sort_hot".into())));
            assert!(query.contains(&("limit".into(), "10".into())));
            assert!(query.contains(&("client_req_type".into(), "3".into())));
            assert!(query.contains(&("selected_item_from_front_page".into(), "0".into())));
        }
        let query = category_params(
            &json!({"id":"0","gender":"0"}),
            &json!({"words":"word_num_gte100","status":"creation_status_end","sort":"sort_score"}),
            2,
        )
        .unwrap();
        assert!(query.contains(&(
            "selected_items".into(),
            "word_num_gte100,creation_status_end".into()
        )));
        assert!(query.contains(&("selected_order".into(), "new_sort_score".into())));
        assert!(query.contains(&("offset".into(), "10".into())));
        assert!(query.contains(&("client_req_type".into(), "2".into())));
        let specific = category_params(&json!({"id":"7","gender":"1"}), &json!({}), 1).unwrap();
        assert!(
            !specific
                .iter()
                .any(|(key, _)| key == "category_new_page_715")
        );
    }
    #[test]
    fn categories_deduplicate_ids_and_keep_labels() {
        let data = json!({"category_tab_data":{"cell_data":[{"cell_name":"主题","atom_data":[{"category_data":{"name":"玄幻","category_id":7}},{"category_data":{"name":"玄幻","category_id":7}}]}]}});
        let menu = category_menu(&data, "1");
        assert_eq!(menu.len(), 3);
        assert_eq!(menu[2]["title"], "玄幻");
    }
}
