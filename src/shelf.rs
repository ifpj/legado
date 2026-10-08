use crate::{
    api::{Api, now, params},
    model,
};
use anyhow::{Result, ensure};
use serde_json::{Value, json};
fn contains(data: &Value, id: &str) -> bool {
    data["book_shelf_info_all"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|r| model::s(r, "book_id") == id && model::s(r, "book_type") == "0")
}
pub fn body(id: &str) -> Value {
    json!({"identify_data":[{"asterisked":false,"modify_time":now(),"book_type":0,"book_id":id}],"add_book_source":0})
}
pub async fn change(api: &mut Api, args: &Value) -> Result<Value> {
    ensure!(api.account.is_some(), "请先在书源登录界面登录番茄账号");
    let id = model::book_id(&args["book"])?;
    let action = model::s(args, "action");
    ensure!(
        matches!(action.as_str(), "add" | "remove"),
        "书架操作请选择 add 或 remove"
    );
    let before = api
        .get(
            "/reading/bookapi/bookshelf/list/v",
            params(&[("server_time", "0")]),
        )
        .await?;
    let exists = contains(&before, &id);
    let desired = action == "add";
    if exists == desired {
        return Ok(json!({"status":"unchanged","bookId":id,"onShelf":exists,"raw":before}));
    }
    if model::boolean(&args["dryRun"]) {
        return Ok(
            json!({"status":"dry_run","bookId":id,"onShelf":exists,"desired":desired,"raw":before}),
        );
    }
    let path = if desired {
        "/reading/bookapi/bookshelf/add/v"
    } else {
        "/reading/bookapi/bookshelf/delete/v"
    };
    let result = api.api(path, vec![], Some(body(&id))).await?;
    let after = api
        .get(
            "/reading/bookapi/bookshelf/list/v",
            params(&[("server_time", "0")]),
        )
        .await?;
    ensure!(
        contains(&after, &id) == desired,
        "番茄未保存目标书架状态，请稍后重试"
    );
    Ok(json!({"status":"updated","bookId":id,"onShelf":desired,"raw":after,"upstream":result}))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bookshelf_ids_and_types_are_exact() {
        let b = body("6883748331202284558");
        assert_eq!(b["identify_data"][0]["book_id"], "6883748331202284558");
        assert_eq!(b["identify_data"][0]["book_type"], 0);
        assert!(contains(
            &json!({"book_shelf_info_all":[{"book_id":"6883748331202284558","book_type":0}]}),
            "6883748331202284558"
        ));
    }
}
