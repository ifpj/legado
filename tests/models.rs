use cf_gateway::{css, http_tasks, native_input, network::USER_AGENT, raster, static_data, web};
use serde_json::{json, Value};
#[test]
fn numeric_wire_strings_use_shortest_even_rounding() {
    assert_eq!(
        cf_gateway::math::number(1125899906842616.25),
        "1125899906842616.2"
    );
    assert_eq!(cf_gateway::math::number(1e-6), "0.000001");
    assert_eq!(cf_gateway::math::number(-0.0), "0");
    assert_eq!(cf_gateway::math::number(1e20), "100000000000000000000");
}
#[test]
fn css_independent_vectors() {
    let cases: Value =
        serde_json::from_str(include_str!("fixtures/css-serialization-vectors.json")).unwrap();
    for case in cases.as_array().unwrap() {
        assert_eq!(
            serde_json::to_value(css::stylesheet(case["source"].as_str().unwrap()).unwrap())
                .unwrap(),
            case["cssRules"],
            "{}",
            case["source"]
        );
    }
}
#[test]
fn software_pixels_are_input_sensitive() {
    let case: Value =
        serde_json::from_str(include_str!("fixtures/raster-scene-input.json")).unwrap();
    let pixels = raster::scene(&case["shapes"], &case["gradients"])
        .unwrap()
        .pixels;
    assert_eq!(pixels.len(), 48 * 48 * 4);
    assert_eq!(raster::windows_digest(&pixels), case["expectedDigest"]);
    let mut shapes = case["shapes"].clone();
    let shape = shapes
        .as_array_mut()
        .unwrap()
        .last_mut()
        .unwrap()
        .as_array_mut()
        .unwrap();
    let last = shape.last_mut().unwrap();
    *last = json!(last.as_f64().unwrap() + 10.);
    assert_ne!(
        raster::scene(&shapes, &case["gradients"]).unwrap().pixels,
        pixels
    );
}
#[test]
fn web_source_embeds_address_as_data() {
    let source = web::source("https://gateway.example:1443/path", "a\"\\\n").unwrap();
    assert!(source.contains("https://gateway.example:1443/path"));
    assert!(source.contains("var CF_TOKEN = \"a\\\"\\\\\\n\";"));
    assert!(web::source("https://example.com/?secret=x", "").is_err());
    assert!(source.contains("https://twkan.com/cf-gateway-relay"));
    assert!(USER_AGENT.contains("Chrome/149"));
}
#[test]
fn managed_configuration_is_literal_data() {
    let html = r#"<html><script>window._cf_chl_opt={cType:'managed',cvId:'3',callback:function(){throw 1}};let x=document.createElement('script');x.src='/cdn-cgi/challenge-platform/h/b/orchestrate/chl_page/v1';</script></html>"#;
    let h = json!({"content-type":"text/html","cf-mitigated":"challenge"});
    let c =
        static_data::recognize("https://example.com/", 403, h.as_object().unwrap(), html).unwrap();
    assert_eq!(c.kind, "managed");
    assert_eq!(c.config["cType"], "managed");
    assert!(c.config.get("callback").is_none());
    assert!(c
        .script
        .unwrap()
        .starts_with("https://example.com/cdn-cgi/"));
}
#[test]
fn background_loader_data_is_read_without_evaluating_its_script() {
    let html = r#"<html><body>business document<script>function neverCall(){var script=document.createElement('script');script.innerHTML="window.__CF$cv$params={r:'1234567890abcdef',t:'MTc5MjYwMDAwMA=='};throw new Error('must not execute')";}</script></body></html>"#;
    let value = static_data::assignment(html, "__CF$cv$params")
        .unwrap()
        .unwrap();
    assert_eq!(value["r"], "1234567890abcdef");
    assert!(
        static_data::assignment("function x(){throw 1}", "_cf_chl_opt")
            .unwrap()
            .is_none()
    );
}
#[test]
fn cors_visibility_is_required() {
    assert_eq!(
        http_tasks::integrity(204, json!({"cf-chl-peek":"token"}).as_object().unwrap()),
        json!({"hkqe2":"fetch_error"})
    );
    assert_eq!(
        http_tasks::integrity(
            204,
            json!({"access-control-allow-origin":"*","cf-chl-peek":"token"})
                .as_object()
                .unwrap()
        ),
        json!({"hkqe2":"no_token"})
    );
    assert_eq!(http_tasks::integrity(204,json!({"access-control-allow-origin":"*","access-control-expose-headers":"cf-chl-peek","cf-chl-peek":"token"}).as_object().unwrap()),json!({"hkqe2":"ok"}));
}
#[test]
fn generated_geometry_uses_current_declarations() {
    let html="<style>#root {height:65px;display:flex;padding:0 8px;border:1px solid #ddd}.check input {position:absolute;inset:0;opacity:0;width:100%;height:100%}.check .box {width:24px;height:24px;border:2px solid #444;grid-area:1/1}@keyframes test {100% {transform:rotate(360deg)}}</style>";
    let a = native_input::proof(html).unwrap();
    let b = native_input::proof(&html.replace("24px", "30px")).unwrap();
    assert_ne!(a["YBguh4"], b["YBguh4"]);
    assert_eq!(a["YBguh4"]["HuPfz3"], "0");
    assert!(native_input::proof("<style>body{color:red}</style>").is_err());
}
