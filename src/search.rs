//! The APK's book tab supplies its own matching title and opaque pagination.
use crate::{
    api::{number, params},
    model,
};
use anyhow::{Result, anyhow, ensure};
use serde_json::{Value, json};
use std::collections::HashSet;

pub const PATH: &str = "/reading/bookapi/search/tab/v";

pub fn request_params(query: &str, cursor: &Value, logged_in: bool) -> Vec<(String, String)> {
    params(&[
        ("query", query),
        ("tab_type", "3"),      // SearchTabType.Book
        ("search_source", "1"), // SearchSource.BOOKSTORE
        ("bookshelf_search_plan", "4"),
        ("user_is_login", if logged_in { "1" } else { "0" }),
        ("use_correct", "false"),
        ("corrected_query", &model::s(cursor, "correctedQuery")),
        ("offset", &number(&cursor["offset"]).to_string()),
        ("search_id", &model::s(cursor, "searchId")),
        ("passback", &model::s(cursor, "passback")),
    ])
}

pub fn book_tab(response: &Value) -> Result<&Value> {
    response["search_tabs"]
        .as_array()
        .and_then(|tabs| tabs.iter().find(|tab| number(&tab["tab_type"]) == 3))
        .ok_or_else(|| anyhow!("官方搜索接口未返回书籍标签"))
}

pub fn next_cursor(tab: &Value) -> Result<Value> {
    let more = model::boolean(&tab["has_more"]);
    ensure!(
        !more
            || tab
                .get("next_offset")
                .is_some_and(|offset| offset.as_u64().is_some()),
        "官方搜索接口未返回分页偏移"
    );
    Ok(
        json!({"offset":number(&tab["next_offset"]),"searchId":model::s(tab,"search_id"),
        "passback":model::s(tab,"passback"),"correctedQuery":model::s(tab,"corrected_query"),"done":!more}),
    )
}

pub fn books(tab: &Value, seen: &mut Vec<String>) -> Vec<model::Book> {
    let mut ids: HashSet<String> = seen.iter().cloned().collect();
    let mut books = vec![];
    for cell in tab["data"].as_array().into_iter().flatten() {
        let Some(rows) = cell["book_data"].as_array() else {
            continue;
        };
        for info in rows {
            let id = model::s(info, "book_id");
            if model::s(info, "book_type") != "0"
                || !matches!(model::s(info, "type").as_str(), "" | "0")
                || model::input_book_id(&id).is_none()
                || !ids.insert(id.clone())
            {
                continue;
            }
            seen.push(id);
            books.push(model::search_book(info, cell, rows.len() == 1));
        }
    }
    books
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn official_matching_title_and_aliases_keep_the_same_book_identity() {
        let cell = json!({"show_type":110,"search_high_light":{"title":{"text":"原名（别名：新名）","rich_text":"<em>原名</em>（别名：新名）"}},
            "book_data":[{"book_id":"7519457513788427326","book_type":"0","type":"0","book_name":"新名","original_book_name":"原名","alias_name":"原名","book_short_name":"短名"}]});
        let items = books(&json!({"data":[cell.clone()]}), &mut vec![]);
        let book = serde_json::to_value(&items[0]).unwrap();
        assert_eq!(book["name"], "原名（别名：新名）");
        assert_eq!(
            book["bookUrl"],
            "https://fanqienovel.com/page/7519457513788427326"
        );
        assert!(book["intro"].as_str().unwrap().contains("原书名：原名"));
        assert!(book["intro"].as_str().unwrap().contains("短名：短名"));
        let variable: Value = serde_json::from_str(book["variable"].as_str().unwrap()).unwrap();
        let raw: Value = serde_json::from_str(variable["fanqie"].as_str().unwrap()).unwrap();
        assert_eq!(raw["book"]["book_name"], "新名");
        assert_eq!(raw["search"], cell);
        assert_eq!(
            serde_json::to_value(model::book(&raw["book"], None)).unwrap()["name"],
            "新名"
        );
    }

    #[test]
    fn title_falls_back_without_using_rich_html_or_a_collection_heading() {
        let info = json!({"book_id":"123","book_name":"书名"});
        let highlight =
            json!({"search_high_light":{"title":{"rich_text":"<em>不可作为书名</em>"}}});
        assert_eq!(
            serde_json::to_value(model::search_book(&info, &highlight, true)).unwrap()["name"],
            "书名"
        );
        assert_eq!(
            serde_json::to_value(model::search_book(
                &info,
                &json!({"cell_name":"展示标题"}),
                true
            ))
            .unwrap()["name"],
            "展示标题"
        );
        assert_eq!(
            serde_json::to_value(model::search_book(
                &info,
                &json!({"cell_name":"推荐合集"}),
                false
            ))
            .unwrap()["name"],
            "书名"
        );
    }

    #[test]
    fn chooses_book_tab_and_preserves_exact_offsets_and_passback() {
        let response = json!({"selected_tab_idx":0,"search_tabs":[{"tab_type":2,"data":null},
            {"tab_type":3,"next_offset":37,"has_more":true,"search_id":"session","passback":"opaque-token","corrected_query":"correction","data":[]} ]});
        let tab = book_tab(&response).unwrap();
        let cursor = next_cursor(tab).unwrap();
        let request = request_params("query", &cursor, true);
        for (key, value) in [
            ("offset", "37"),
            ("passback", "opaque-token"),
            ("search_id", "session"),
            ("tab_type", "3"),
            ("corrected_query", "correction"),
            ("user_is_login", "1"),
        ] {
            assert!(request.contains(&(key.into(), value.into())));
        }
        assert_eq!(cursor["done"], false);
        assert_eq!(
            next_cursor(&json!({"has_more":false})).unwrap()["done"],
            true
        );
        assert!(next_cursor(&json!({"has_more":true})).is_err());
        assert!(book_tab(&json!({"search_tabs":[]})).is_err());
    }

    #[test]
    fn excludes_non_novels_and_repeated_books_without_losing_metadata() {
        let tab = json!({"data":[{"book_data":[
            {"book_id":"123","book_type":"0","book_name":"already seen"},
            {"book_id":"456","book_type":"0","book_name":"new"},
            {"book_id":"456","book_type":"0","book_name":"duplicate"},
            {"book_id":"789","book_type":"1","book_name":"audio"}
        ]},{"show_type":130,"author_data":{"name":"author"}}]});
        let mut seen = vec!["123".into()];
        let rows = books(&tab, &mut seen);
        assert_eq!(rows.len(), 1);
        assert_eq!(serde_json::to_value(&rows[0]).unwrap()["name"], "new");
        assert_eq!(seen, vec!["123", "456"]);
        assert!(books(&tab, &mut seen).is_empty());
    }
}
