use super::*;
use crate::network::{Dns, HttpResponse, NetworkConfig};
use std::time::Duration;

fn session() -> Arc<Session> {
    Arc::new(Session::new(Dns::new(&NetworkConfig::default()).unwrap(), None).unwrap())
}
fn outcome(text: String) -> Outcome {
    Outcome {
        response: HttpResponse {
            status: 404,
            url: "https://example.com/fixture".into(),
            headers: serde_json::Map::new(),
            content: text.as_bytes().to_vec(),
            download_size: text.len(),
            text: text.into(),
        },
        details: json!({"type":"none"}),
        solved: false,
    }
}

#[tokio::test]
async fn html_and_value_move_the_original_body_and_image_moves_original_bytes() {
    let start = Instant::now();
    let deadline = start + Duration::from_secs(5);
    for format in [Format::Html, Format::Value] {
        let text = "<html>测试😀</html>".repeat(1000);
        let pointer = text.as_ptr() as usize;
        let expected = text.clone();
        let reply = prepare(
            outcome(text),
            session(),
            format,
            false,
            false,
            12.5,
            start,
            deadline,
        )
        .await
        .unwrap();
        assert_eq!(reply.value["status"], 404);
        assert_eq!(reply.value["timings"]["queueMs"], 12.5);
        assert!(reply.value["timings"]["totalMs"].as_f64().unwrap() >= 0.0);
        if format == Format::Html {
            assert!(reply.value.get("body").is_none());
            assert_eq!(reply.bytes.as_ptr() as usize, pointer);
            assert_eq!(reply.bytes, expected.as_bytes());
        } else {
            assert!(reply.bytes.is_empty());
            assert_eq!(
                reply.value["body"].as_str().unwrap().as_ptr() as usize,
                pointer
            );
            assert_eq!(reply.value["body"], expected);
        }
    }
    let mut image = outcome("ignored fixture text".into());
    image.response.status = 200;
    image.response.content = b"\x89PNG\r\n\x1a\nsynthetic".to_vec();
    let pointer = image.response.content.as_ptr() as usize;
    let reply = prepare(
        image,
        session(),
        Format::Image,
        false,
        false,
        0.,
        start,
        deadline,
    )
    .await
    .unwrap();
    assert!(reply.value.get("body").is_none());
    assert_eq!(reply.bytes.as_ptr() as usize, pointer);
    assert_eq!(reply.value["bytes"], reply.bytes.len());
    assert_eq!(reply.value["imageType"], "image/png");
}

#[tokio::test]
async fn json_wire_preserves_escaping_metadata_and_export_without_a_body() {
    let start = Instant::now();
    let deadline = start + Duration::from_secs(5);
    let body = "<html>中文😀\n\t\"\\\u{0000}</html>".repeat(50_000);
    let before_preparation = start.elapsed().as_secs_f64() * 1000.;
    let reply = prepare(
        outcome(body.clone()),
        session(),
        Format::Json,
        false,
        false,
        2.5,
        start,
        deadline,
    )
    .await
    .unwrap();
    let decoded: Value = serde_json::from_slice(&reply.bytes).unwrap();
    assert_eq!(decoded["body"], body);
    assert_eq!(decoded["timings"], reply.value["timings"]);
    assert!(decoded["timings"]["totalMs"].as_f64().unwrap() >= before_preparation);
    assert_eq!(decoded["status"], 404);
    assert_eq!(decoded["contentCached"], false);
    assert!(reply.value.get("body").is_none());
    for format in [Format::Value, Format::Json] {
        let reply = prepare(
            outcome(body.clone()),
            session(),
            format,
            false,
            true,
            0.,
            start,
            deadline,
        )
        .await
        .unwrap();
        let value = if format == Format::Json {
            serde_json::from_slice(&reply.bytes).unwrap()
        } else {
            reply.value
        };
        assert!(value.get("body").is_none());
        assert_eq!(value["credentials"]["url"], "https://example.com/fixture");
    }
}

#[tokio::test]
async fn preparing_every_response_format_obeys_the_existing_deadline() {
    let start = Instant::now() - Duration::from_secs(1);
    for format in [Format::Value, Format::Json, Format::Html, Format::Image] {
        let result = prepare(
            outcome("fixture".into()),
            session(),
            format,
            false,
            false,
            0.,
            start,
            start,
        )
        .await;
        let error = match result {
            Ok(_) => panic!("expired preparation succeeded"),
            Err(error) => error,
        };
        assert_eq!(error.code, "TIMEOUT");
        assert_eq!(error.status, 504);
    }
}
