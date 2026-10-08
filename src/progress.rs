//! Chapter positions are always read from the upstream. Only locks are retained.
use crate::{
    api::{self, Api},
    model,
};
use anyhow::{Result, ensure};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, Weak},
};

#[derive(Clone, Default)]
pub struct Locks(Arc<Mutex<HashMap<String, Weak<tokio::sync::Mutex<()>>>>>);
impl Locks {
    pub fn get(&self, uid: &str, book: &str) -> Arc<tokio::sync::Mutex<()>> {
        let mut locks = self.0.lock().unwrap();
        locks.retain(|_, v| v.strong_count() > 0);
        let key = format!("{uid}:{book}");
        if let Some(lock) = locks.get(&key).and_then(Weak::upgrade) {
            return lock;
        }
        let lock = Arc::new(tokio::sync::Mutex::new(()));
        locks.insert(key, Arc::downgrade(&lock));
        lock
    }
}
pub fn book_ids(id: &str) -> Result<Value> {
    ensure!(id.parse::<u64>().is_ok(), "书籍 ID 无效");
    // get/v requires JSON integers. Avoid an f64 round trip for 19-digit IDs.
    Ok(json!({"book_ids":{"0":[serde_json::from_str::<Value>(id)?]}}))
}
pub async fn get_raw(api: &mut Api, id: &str) -> Result<Value> {
    ensure!(
        api.account.as_ref().is_some_and(|a| !a.uid.is_empty()),
        "请先在书源登录界面登录番茄账号"
    );
    let data = api
        .api(
            "/reading/bookapi/read_progress/get/v",
            vec![],
            Some(book_ids(id)?),
        )
        .await?;
    Ok(data
        .as_array()
        .and_then(|rows| {
            rows.iter().find(|r| {
                model::s(r, "book_id") == id
                    && model::s(r, "book_type") == "0"
                    && api::number(&r["item_id"]) > 0
            })
        })
        .cloned()
        .unwrap_or(Value::Null))
}
pub fn position(raw: &Value) -> Value {
    if raw.is_null() {
        return Value::Null;
    }
    json!({"bookId":model::s(raw,"book_id"),"itemId":model::s(raw,"item_id"),"chapterTitle":model::first(raw,&["title","chapter_title"]).map(model::string).unwrap_or_default(),"timestampMs":api::number(&raw["read_timestamp_ms"])})
}
pub async fn get(api: &mut Api, book: &Value) -> Result<Value> {
    let id = model::book_id(book)?;
    let raw = get_raw(api, &id).await?;
    with_position(api, &id, raw).await
}
async fn with_position(api: &mut Api, id: &str, raw: Value) -> Result<Value> {
    let mut pos = position(&raw);
    if !pos.is_null() && !model::s(&pos, "itemId").is_empty() {
        let toc = api
            .get(
                "/reading/bookapi/directory/all_items/v",
                api::params(&[("book_id", id), ("filter_copyright_page", "false")]),
            )
            .await?;
        let chapters = model::chapters(&toc, None)?;
        let item = model::s(&pos, "itemId");
        if let Some((index, chapter)) = chapters.iter().enumerate().find(|(_, c)| {
            serde_json::to_value(c)
                .ok()
                .is_some_and(|v| model::s(&v, "url").ends_with(&format!("/reader/{item}")))
        }) {
            pos["chapterIndex"] = json!(index);
            pos["chapterTitle"] = serde_json::to_value(chapter)?["title"].clone();
        }
    }
    Ok(json!({"position":pos,"raw":raw}))
}
pub fn upload_body(id: &str, item: &str, timestamp: u64, index: u64, count: u64) -> Value {
    // A changed chapter starts at its beginning. Equal chapters are never
    // uploaded, preserving the APK's paragraph-level position.
    json!({"books":[{"book_id":id,"item_id":item,"book_type":0,"read_timestamp_ms":timestamp,
        "progress_rate":if count>0 {format!("{}",index as f64/count as f64)} else {"0".into()},
        "item_progress_rate":"0","page_progress_rate":0.0,"paragraph_id":"0","paragraph_offset":0,
        "check_timestamp":true,"is_local_book":false,"is_listen_mode":false}],"write_mode":1})
}
pub fn decision(remote: &Value, item: &str, base_timestamp: u64) -> &'static str {
    if !remote.is_null()
        && model::s(remote, "item_id") == item
        && api::number(&remote["read_timestamp_ms"]) >= base_timestamp
    {
        "unchanged"
    }
    // Compare to the last cloud observation, not clocks from different devices.
    else if api::number(&remote["read_timestamp_ms"]) > base_timestamp {
        "conflict"
    } else {
        "upload"
    }
}
pub fn upload_timestamp(event_timestamp: u64, remote: &Value, base_timestamp: u64) -> u64 {
    // A second chapter event can be queued before the first upload finishes.
    // Its device time may precede that upload's server-assigned time even
    // though it is the user's next chapter. Advance the observed timestamp;
    // decision() still rejects a cloud change newer than the client's base.
    event_timestamp
        .max(api::number(&remote["read_timestamp_ms"]).saturating_add(1))
        .max(base_timestamp.saturating_add(1))
}
fn upload_result(submitted: Value, response: Value, verified: Result<Value>) -> Value {
    // SUCCESS acknowledges the write; the read endpoint can still return its
    // previous value. Only a matching item AND timestamp confirm this write.
    match verified {
        Ok(raw)
            if model::s(&raw, "item_id") == model::s(&submitted, "itemId")
                && api::number(&raw["read_timestamp_ms"])
                    >= api::number(&submitted["timestampMs"]) =>
        {
            json!({"status":"uploaded","position":position(&raw),"raw":raw,
                "upload":response,"verification":{"status":"confirmed"}})
        }
        Ok(raw)
            if api::number(&raw["read_timestamp_ms"]) >= api::number(&submitted["timestampMs"]) =>
        {
            json!({"status":"conflict","position":position(&raw),"raw":raw,
                "upload":response,"verification":{"status":"conflict"}})
        }
        Ok(raw) => json!({"status":"accepted","position":submitted,"raw":raw,
            "upload":response,"verification":{"status":"pending","observedPosition":position(&raw),
                "message":"上游已受理，进度回读暂未更新，将在后续同步时确认"}}),
        Err(error) => json!({"status":"accepted","position":submitted,"raw":null,
            "upload":response,"verification":{"status":"pending","error":crate::web::safe_error(&error),
                "message":"上游已受理，本次回读失败，将在后续同步时确认"}}),
    }
}
pub async fn put(api: &mut Api, args: &Value) -> Result<Value> {
    let id = model::book_id(&args["book"])?;
    let item = api::chapter_id(&args["chapter"])?;
    let timestamp = api::number(&args["timestampMs"]);
    ensure!(
        timestamp > 0 && timestamp <= api::now() + 300_000,
        "章节阅读时间无效"
    );
    let remote = get_raw(api, &id).await?;
    let action = decision(&remote, &item, api::number(&args["baseTimestampMs"]));
    if action != "upload" {
        let result = if action == "conflict" {
            get(api, &args["book"]).await?
        } else {
            json!({"position":position(&remote),"raw":remote})
        };
        return Ok(json!({"status":action,"position":result["position"],"raw":result["raw"]}));
    }
    let timestamp = upload_timestamp(timestamp, &remote, api::number(&args["baseTimestampMs"]));
    ensure!(timestamp <= api::now() + 300_000, "同步基准时间无效");
    let response = api
        .api(
            "/reading/bookapi/read_progress/upload/v",
            vec![],
            Some(upload_body(
                &id,
                &item,
                timestamp,
                api::number(&args["chapterIndex"]),
                api::number(&args["chapterCount"]),
            )),
        )
        .await?;
    let submitted = json!({"bookId":id,"itemId":item,"timestampMs":timestamp,
        "chapterIndex":api::number(&args["chapterIndex"]),"chapterTitle":model::s(&args["chapter"],"title")});
    let mut result = upload_result(submitted, response, get_raw(api, &id).await);
    if result["status"] == "conflict" {
        // Resolve the actual cloud chapter, without another progress read.
        result["position"] =
            with_position(api, &id, result["raw"].clone()).await?["position"].clone();
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_ids_and_chapter_start() {
        let body = book_ids("6883748331202284558").unwrap();
        assert_eq!(body["book_ids"]["0"][0].to_string(), "6883748331202284558");
        assert!(body["book_ids"]["0"][0].is_number());
        let upload = upload_body("6883748331202284558", "6883749008234250760", 1000, 2, 10);
        assert_eq!(upload["books"][0]["book_id"], "6883748331202284558");
        assert_eq!(upload["books"][0]["paragraph_offset"], 0);
        assert_eq!(upload["write_mode"], 1);
    }
    #[test]
    fn newer_cloud_and_equal_chapter_preserve_position() {
        let raw = json!({"item_id":"2","read_timestamp_ms":2000,"paragraph_offset":123});
        assert_eq!(decision(&raw, "2", 1000), "unchanged");
        assert_eq!(decision(&raw, "3", 1000), "conflict");
        assert_eq!(decision(&raw, "3", 2000), "upload");
        assert_eq!(decision(&Value::Null, "1", 0), "upload");
    }
    #[test]
    fn queued_chapter_and_slow_clock_do_not_conflict_with_fresh_base() {
        let raw = json!({"item_id":"2","read_timestamp_ms":2000});
        assert_eq!(decision(&raw, "3", 2000), "upload");
        assert_eq!(upload_timestamp(1500, &raw, 2000), 2001);
        assert_eq!(upload_timestamp(3000, &raw, 2000), 3000);
        assert_eq!(decision(&raw, "3", 1999), "conflict");
    }
    #[test]
    fn returning_to_a_chapter_before_a_pending_write_is_visible_must_upload() {
        // A -> B (accepted but not visible) -> A. Seeing the old A is not
        // evidence that the user's latest A event has already been stored.
        let old_a = json!({"item_id":"1","read_timestamp_ms":1000});
        assert_eq!(decision(&old_a, "1", 2000), "upload");
        assert_eq!(upload_timestamp(1500, &old_a, 2000), 2001);
    }
    #[test]
    fn account_book_lock_shared_only_for_same_pair() {
        let locks = Locks::default();
        let a = locks.get("1", "2");
        assert!(Arc::ptr_eq(&a, &locks.get("1", "2")));
        assert!(!Arc::ptr_eq(&a, &locks.get("2", "2")));
        assert!(!Arc::ptr_eq(&a, &locks.get("1", "3")));
    }
    #[test]
    fn successful_write_with_stale_or_failed_read_is_pending_not_failure() {
        let submitted = json!({"bookId":"1","itemId":"3","timestampMs":2001,"chapterIndex":2});
        let raw = json!({"book_id":"1","item_id":"2","read_timestamp_ms":2000});
        let pending = upload_result(submitted.clone(), json!([]), Ok(raw.clone()));
        assert_eq!(pending["status"], "accepted");
        assert_eq!(pending["position"], submitted);
        assert_eq!(pending["raw"], raw);
        assert_eq!(pending["verification"]["status"], "pending");
        // Using the accepted timestamp as the next base prevents the relay's
        // own delayed write from appearing to be another device's change.
        let caught_up = json!({"item_id":"3","read_timestamp_ms":2001});
        assert_eq!(
            decision(
                &caught_up,
                "4",
                api::number(&pending["position"]["timestampMs"])
            ),
            "upload"
        );
        let failed_read = upload_result(
            submitted.clone(),
            json!([]),
            Err(anyhow::anyhow!("read unavailable")),
        );
        assert_eq!(failed_read["status"], "accepted");
        assert_eq!(failed_read["position"], submitted);
        assert!(failed_read["verification"]["error"].is_string());
        let absent = upload_result(submitted, json!([]), Ok(Value::Null));
        assert_eq!(absent["status"], "accepted");
    }
    #[test]
    fn verification_requires_this_write_and_retains_newer_cloud_conflicts() {
        let submitted = json!({"itemId":"3","timestampMs":2001});
        let old_same_item = json!({"item_id":"3","read_timestamp_ms":1999});
        assert_eq!(
            upload_result(submitted.clone(), json!([]), Ok(old_same_item))["status"],
            "accepted"
        );
        let confirmed = json!({"item_id":"3","read_timestamp_ms":2001});
        assert_eq!(
            upload_result(submitted.clone(), json!([]), Ok(confirmed))["verification"]["status"],
            "confirmed"
        );
        let other_device = json!({"item_id":"4","read_timestamp_ms":2002});
        let conflict = upload_result(submitted, json!([]), Ok(other_device.clone()));
        assert_eq!(conflict["status"], "conflict");
        assert_eq!(conflict["raw"], other_device);
    }
}
