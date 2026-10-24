use super::*;

#[test]
fn disabled_capture_does_not_generate_or_serialize_payloads() {
    Engine::default()
        .capture_generated(".payload.json", || panic!("capture is disabled"))
        .unwrap();
    Engine::default()
        .capture_generated(".error.json", || panic!("error reply must also stay lazy"))
        .unwrap();
}

#[test]
fn enabled_capture_keeps_the_content_hash_and_propagates_generation_errors() {
    let dir = std::env::temp_dir().join(format!("cf-capture-test-{}", uuid::Uuid::new_v4()));
    let engine = Engine {
        capture_dir: Some(dir.clone()),
    };
    let payload = json!({"fixture":"非敏感合成数据","value":17});
    let bytes = serde_json::to_vec(&payload).unwrap();
    let path = dir.join(format!("{}.payload.json", models::sha(&bytes)));
    for _ in 0..2 {
        engine
            .capture_generated(".payload.json", || Ok(serde_json::to_vec(&payload)?))
            .unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }
    let error = engine
        .capture_generated(".error.json", || {
            Err(Error::new("FIXTURE_ERROR", "fixture"))
        })
        .unwrap_err();
    assert_eq!(error.code, "FIXTURE_ERROR");
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
    std::fs::remove_file(path).unwrap();
    std::fs::remove_dir(dir).unwrap();
}
