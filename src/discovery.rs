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
pub const RANKS: &[(&str, &str)] = &[
    ("巅峰榜", "200"),
    ("口碑榜", "104"),
    ("阅读榜", "111"),
    ("高分榜", "115"),
    ("追更榜", "109"),
    ("热评榜", "110"),
    ("黑马榜", "102"),
    ("人气榜", "146"),
    ("推荐榜", "101"),
    ("完本榜", "100"),
    ("热搜榜", "103"),
    ("新书榜", "204"),
    ("短篇榜", "601"),
    ("抖音榜", "156"),
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
    let mut selected = filters(preferences)?;
    if let Some(sub) = args.get("sub").map(model::string).filter(|s| s != "0") {
        ensure!(sub.parse::<u64>().is_ok(), "分类标签无效");
        selected.push_str(&format!(",cate_{sub}"));
    }
    Ok(params(&[
        ("gender", &g),
        ("query_gender", &g),
        ("genre_type", "0"),
        ("category_id", &id),
        ("selected_items", &selected),
        ("source", "front_category"),
        ("page_version", "2"),
        ("limit", "20"),
        ("offset", &((page - 1) * 20).to_string()),
    ]))
}
pub fn rank_params(args: &Value, page: usize) -> Result<Vec<(String, String)>> {
    let g = gender(args)?;
    let algo = model::s(args, "algo");
    ensure!(RANKS.iter().any(|(_, id)| *id == algo), "榜单类型无效");
    let mut p = params(&[
        ("gender", &g),
        ("gender_list_type", &g),
        ("algo_type", &algo),
        ("change_type", "1"),
        ("cell_id", "7098235271900037133"),
        ("limit", "20"),
        ("offset", &(page - 1).to_string()),
    ]);
    if algo == "200" || algo == "601" {
        p.push(("tab_type".into(), "2".into()));
        if algo == "200" {
            p.push(("support_gender_list".into(), "true".into()));
            p.push((
                "specific_sub_tab_type".into(),
                (g.parse::<u32>()? + 4).to_string(),
            ));
        }
    }
    Ok(p)
}
fn item(title: &str, url: &str, heading: bool) -> Value {
    json!({"title":title,"url":url,"style":{"layout_flexGrow":1,"layout_flexBasisPercent":if heading {1.0}else{0.23}}})
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
    entries.extend(RANKS.iter().map(|(title, algo)| {
        item(
            title,
            &format!("fanqie://ranking?gender={g}&algo={algo}"),
            false,
        )
    }));
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
    Ok(json!({"entries":entries,"filters":{"words":WORDS,"status":STATUS,"sort":SORT},"raw":data}))
}
pub async fn list(
    api: &mut Api,
    url: &str,
    preferences: &Value,
    page: usize,
) -> Result<Vec<model::Book>> {
    let args = parse_url(url)?;
    let data = if url.starts_with("fanqie://category?") {
        api.get(
            "/reading/bookapi/new_category/landing/v",
            category_params(&args, preferences, page)?,
        )
        .await?
    } else {
        api.get(
            "/reading/bookapi/bookmall/cell/change/v1",
            rank_params(&args, page)?,
        )
        .await?
    };
    // Ranking APIs may always return the same fixed top list. Subsequent
    // calls are still live; guard against repeating page one forever.
    if page > 1
        && !model::boolean(&data["has_more"])
        && number(&data["next_offset"]) == 0
        && data.get("cell_view").is_some()
    {
        return Ok(vec![]);
    }
    Ok(model::book_rows(&data)
        .iter()
        .map(|(v, c)| model::book(v, c.as_ref()))
        .collect())
}
#[cfg(test)]
mod tests {
    use super::*;
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
        assert!(rank_params(&json!({"gender":1,"algo":"9999"}), 1).is_err());
    }
    #[test]
    fn categories_deduplicate_ids_and_keep_labels() {
        let data = json!({"category_tab_data":{"cell_data":[{"cell_name":"主题","atom_data":[{"category_data":{"name":"玄幻","category_id":7}},{"category_data":{"name":"玄幻","category_id":7}}]}]}});
        let menu = category_menu(&data, "1");
        assert_eq!(menu.len(), 3);
        assert_eq!(menu[2]["title"], "玄幻");
    }
}
