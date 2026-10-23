//! Assertions which need injected clocks, typed callbacks or native ownership.
use crate::*;
use serde_json::json;
#[test]
fn clock_samples_are_measured_and_bounded() {
    let mut samples = [0., 0., 0., 0.5, 1., 1.25, 2., 2.125, 3., 3.5].into_iter();
    assert_eq!(
        declared::clock(5, || samples.next().unwrap()).unwrap(),
        json!({"clAR9":"0.125"})
    );
    assert_eq!(declared::clock(5, || 0.).unwrap(), json!({"clAR9":"1"}));
    assert!(declared::clock(0, || panic!("must not sample")).is_err());
    assert!(declared::clock(5001, || panic!("must not sample")).is_err());
}
#[test]
fn self_check_failures_are_recomputed() {
    let good = models::self_checks(&[|| true], 2, 2, 1).unwrap();
    let bad = models::self_checks(&[|| false], 2, 2, 1).unwrap();
    assert_eq!(good["GMYd6"], json!([]));
    assert_eq!(bad["GMYd6"], json!([0, 1]));
    assert_eq!(good["fayBq4"].as_array().unwrap().len(), 2);
    assert!(models::self_checks(&[], 2, 2, 1).is_err());
    assert!(models::self_checks(&[|| true], 10000, 2, 1).is_err());
}
#[test]
fn finite_fragments_escape_once_and_reject_other_forms() {
    assert_eq!(
        semantics::fragment("<p>A</div><p>B</p>").unwrap(),
        "<p>A</p><p>B</p>"
    );
    assert_eq!(
        semantics::fragment("<div data-foo=\"&#34;\"></div>").unwrap(),
        "<div data-foo=\"&quot;\"></div>"
    );
    assert!(semantics::fragment("<script>bad()</script>").is_err());
    assert!(semantics::fragment(&"x".repeat(1025)).is_err());
}
#[test]
fn literal_reader_never_evaluates_calls_or_functions() {
    for source in ["", "'", "\"", "`"] {
        assert!(static_data::unquote(source).is_err());
    }
    assert_eq!(
        static_data::literal_data(r#"{key:'a\x2fb',arr:[true,null,0x10,],tpl:`中文`,}"#).unwrap(),
        json!({"key":"a/b","arr":[true,null,16],"tpl":"中文"})
    );
    for source in [
        "{x:process.exit()}",
        "{x:1+1}",
        "{x:`a${danger()}`}",
        "{x:function(){}}",
    ] {
        assert!(static_data::literal_data(source).is_err());
    }
}
#[test]
fn compiled_layouts_contain_no_wire_credentials_and_rehydrate_current_parameters() {
    let layouts: serde_json::Value =
        serde_json::from_str(include_str!("../profiles/protocol-layouts-v1.json")).unwrap();
    assert_eq!(layouts["version"], 1);
    for row in layouts["layouts"].as_array().unwrap() {
        let plan = data::normalize(&row["plan"]).unwrap();
        assert!(plan["alphabet"].is_null() || plan["alphabet"] == "");
        assert!(plan["modulus"].is_null() || plan["modulus"] == 0);
        assert!(plan["flow_prefix"].is_null() || plan["flow_prefix"] == "");
    }
    let meta = data::normalize(&layouts["layouts"][0]["plan"]).unwrap();
    let alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+-$";
    let modulus = format!("0x00{}", "8".to_string() + &"0".repeat(255));
    let source = format!("'{alphabet}';'{modulus}';'/fo/1:2:fresh/'");
    let plan = data::Plan::from_layout(&source, meta.clone()).unwrap();
    assert_eq!(plan.alphabet, alphabet);
    assert_eq!(plan.modulus, modulus);
    assert_eq!(plan.flow, "/fo/1:2:fresh/");
    assert!(data::Plan::from_layout(&(source + ";'/fo/3:4:ambiguous/'"), meta).is_err());
}
#[test]
fn html_facts_ignore_script_markup_and_bind_parent_attributes() {
    let facts=html::Facts::read("<html dir=\"ltr\"><head><title>Fresh &amp; current</title><!-- UIPATH_EXTENSION_ID:example --></head><body class=\"frame\"><a href='/'>one</a><a>plain</a><area href='/two'><img><form></form><custom-book></custom-book><script>\"<a href='fake'><img>\";globalThis.executed=true;</script>").unwrap();
    assert_eq!((facts.links, facts.images, facts.forms), (2, 1, 1));
    assert_eq!(facts.title, "Fresh & current");
    assert_eq!(facts.root_attributes, ["dir"]);
    assert_eq!(facts.body_attributes, ["class"]);
    assert!(facts.extension_comment);
    assert_eq!(facts.custom_tags, ["custom-book"]);
}
#[test]
fn proof_budget_still_makes_one_attempt() {
    let value = wire::work_limited(
        "bbbbbbbbbbbbbbbb|1792490000123|0|",
        "suffix",
        0.0001,
        0,
        1,
        2000000,
    )
    .unwrap();
    assert!(value[1] >= 1);
    assert!(value[1] < 2000000);
    assert!(wire::work_limited("foreign", "suffix", 1., 0, 1, 1).is_err());
    assert!(
        wire::work_limited("bbbbbbbbbbbbbbbb|1792490000123|0|", "suffix", 1., 0, 65, 1).is_err()
    );
}
#[test]
fn direct_completion_cannot_schedule_unrequested_work_or_accept_unknown_data() {
    let make = |values: Vec<String>| data::Response {
        raw: vec![],
        program: data::Program {
            sha256: String::new(),
            byte_length: 0,
            code_length: 0,
            strings: vec![],
        },
        literals: values
            .into_iter()
            .enumerate()
            .map(|(i, value)| data::Literal {
                key: None,
                start: i,
                end: i + 1,
                value,
                packed: None,
                continuation: 0,
                pool_offset: 0,
            })
            .collect(),
        packed: vec![],
    };
    let token = "1.".to_string() + &"W".repeat(120);
    let state = "parent-1792532000-1.3.1.1-".to_string() + &"S".repeat(120);
    match completion::next(&make(vec![token.clone(), state.clone()])).unwrap() {
        completion::Next::Done(v) => {
            assert_eq!(v["widgetToken"], token);
            assert_eq!(v["parentState"], state)
        }
        _ => panic!("must not request work"),
    }
    assert_eq!(
        completion::next(&make(vec![])).err().unwrap().code,
        "MANAGED_WORK_UNSUPPORTED"
    );
    assert!(completion::next(&make(vec![token, state, "fail".into()])).is_err());
}
/// Manual local profiling; it deliberately has no network or acceptance flag.
#[test]
#[ignore = "set CF_GATEWAY_PROFILE_SCRIPT to a local captured public initializer"]
fn profile_current_compiler() {
    use base64::Engine;
    let source = std::fs::read_to_string(
        std::env::var("CF_GATEWAY_PROFILE_SCRIPT").expect("initializer path"),
    )
    .unwrap();
    let mut last = std::time::Instant::now();
    let mut rows = Vec::new();
    let mut stage = |name: &str| {
        rows.push((name.to_string(), last.elapsed().as_secs_f64() * 1000.));
        last = std::time::Instant::now();
    };
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_javascript::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(&source, None).unwrap();
    stage("parse");
    let t = constants::Tables::new(tree.root_node(), &source).unwrap();
    stage("tables");
    let f = compiler::format(&t).unwrap();
    stage("format");
    let values = t.strings();
    stage("strings");
    let _ = compiler::script_build(&t);
    stage("build");
    for value in &values {
        if value.len() > 64 && re(r"^[A-Za-z0-9+/]+={0,2}$").is_match(value) {
            if let Ok(raw) = base64::engine::general_purpose::STANDARD.decode(value) {
                if data::program(&raw, &f, false).is_ok() {
                    let _ = compiler::key_material(&t, value, &f);
                }
            }
        }
    }
    stage("key_material");
    let _ = compiler_profiles::dispatcher(&t, &f);
    stage("dispatcher");
    let _ = compiler_profiles::properties(&t, &f);
    stage("properties");
    let _ = compiler_profiles::immediate(&t, &f);
    stage("immediate");
    let _ = compiler_profiles::inputs(&t, &f);
    stage("inputs");
    let _ = compiler_profiles::framed_salt(&t);
    stage("framed");
    let _ = compiler_profiles::integer_tag(&t, &f);
    stage("integer_tag");
    let _ = compiler::declarations(&t);
    stage("declarations");
    eprintln!("{}", serde_json::to_string(&rows).unwrap());
}
