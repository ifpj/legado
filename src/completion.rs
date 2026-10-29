//! Read fresh continuation operands and bounded POST navigation declarations.
use crate::{
    data::{self, Plan, Response},
    re, require,
    static_data::{origin, validate_url},
    string, Result,
};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
fn child_rejected() -> crate::Error {
    crate::Error {
        code: "CHILD_CHALLENGE_REJECTED".into(),
        message: "验证组件返回失败结果".into(),
        status: 422,
        details: json!({"stage":"frame-completion","outcome":"fail"}),
    }
}
fn embedded_token(values: &[&str]) -> Result<String> {
    if values.contains(&"fail") {
        return Err(child_rejected());
    }
    // The standalone complete message contains both token and sToken. They
    // have the same lexical shape; only the value bound to event=complete is
    // the site's verification token. Never select an arbitrary opaque string.
    data::one(values.windows(4).filter_map(|v| {
        (v[..3] == ["cloudflare-challenge", "event", "complete"]
            && (100..=16384).contains(&v[3].len())
            && re(r"^[01]\.[A-Za-z0-9_.-]+$").is_match(v[3]))
        .then_some(v[3])
    }))
}

#[cfg(test)]
mod embedded_completion_tests {
    use super::*;
    #[test]
    fn form_fields_are_bound_to_the_declared_inputs() {
        let names = ["a".repeat(64), "b".repeat(64), "c".repeat(64)];
        let extra = "d".repeat(64);
        let token = format!("{}-1700000000-1.0.1.1-{}", "A".repeat(80), "B".repeat(80));
        let values: Vec<&str> = vec![
            "form",
            "action",
            "method",
            "POST",
            "enctype",
            "application/x-www-form-urlencoded",
            "input",
            "hidden",
            &names[0],
            "input",
            "hidden",
            &names[1],
            "input",
            "hidden",
            &names[2],
            &token,
            &extra,
            &token,
        ];
        assert_eq!(form_fields(&values, 0).unwrap(), ([8, 11, 14], 15));
        let mut duplicate = values.clone();
        duplicate[11] = &names[0];
        assert!(form_fields(&duplicate, 0).is_err());
        let mut wrong = values.clone();
        wrong[9] = "div";
        assert!(form_fields(&wrong, 0).is_err());
        let mut missing = values.clone();
        missing[15] = "short";
        assert!(form_fields(&missing, 0).is_err());
        assert!(form_fields(&[], 0).is_err());
    }
    #[test]
    fn completion_token_is_bound_to_the_complete_message_not_stoken() {
        let token = format!("1.{}", "A".repeat(120));
        let stoken = format!("1.{}", "B".repeat(120));
        let values = [
            "tokenExpired",
            &token,
            "cloudflare-challenge",
            "event",
            "complete",
            &token,
            "sToken",
            &stoken,
        ];
        assert_eq!(embedded_token(&values).unwrap(), token);
        assert!(embedded_token(&["sToken", &stoken]).is_err());
        assert!(
            embedded_token(&["cloudflare-challenge", "event", "complete", &token, "fail"]).is_err()
        );
        assert!(embedded_token(&[
            "cloudflare-challenge",
            "event",
            "complete",
            &token,
            "cloudflare-challenge",
            "event",
            "complete",
            &stoken
        ])
        .is_err());
    }
    #[test]
    fn explicit_child_rejection_is_distinct_from_an_unknown_format() {
        let error = embedded_token(&["fail"]).unwrap_err();
        assert_eq!(error.code, "CHILD_CHALLENGE_REJECTED");
        assert_eq!(error.details["stage"], "frame-completion");
        assert_eq!(error.details["outcome"], "fail");
        let response = Response {
            raw: vec![],
            program: data::Program {
                sha256: String::new(),
                byte_length: 0,
                code_length: 0,
                strings: vec![],
            },
            literals: vec![data::Literal {
                key: None,
                start: 0,
                end: 4,
                value: "fail".into(),
                packed: None,
                continuation: 0,
                pool_offset: 0,
            }]
            .into(),
            packed: vec![],
        };
        assert_eq!(
            response.completion().unwrap_err().code,
            "CHILD_CHALLENGE_REJECTED"
        );
        assert_eq!(
            response.widget_completion().unwrap_err().code,
            "CHILD_CHALLENGE_REJECTED"
        );
    }
}

impl Response {
    pub fn widget_completion(&self) -> Result<Value> {
        let values: Vec<_> = self.literals.iter().map(|l| l.value.as_str()).collect();
        Ok(json!({"widgetToken":embedded_token(&values)?}))
    }
    pub fn completion(&self) -> Result<Value> {
        let values: HashSet<_> = self.literals.iter().map(|l| l.value.as_str()).collect();
        if values.contains("fail") {
            return Err(child_rejected());
        }
        Ok(
            json!({"widgetToken":data::one(values.iter().filter(|v|(100..=16384).contains(&v.len())&&re(r"^1\.[A-Za-z0-9_.-]+$").is_match(v)).copied())?,"parentState":data::one(values.iter().filter(|v|v.len()<=16384&&data::token(v)&&v.contains("-1.3.1.1-")).copied())?}),
        )
    }
    pub fn proof(&self) -> Result<Value> {
        let values = self.values();
        let mut counts = HashMap::<&str, usize>::new();
        for value in &values {
            *counts.entry(value).or_default() += 1;
        }
        let tokens: HashSet<_> = values
            .iter()
            .filter(|v| data::token(v) && v.contains("-1.3.1.1-"))
            .copied()
            .collect();
        let state = data::one(tokens.iter().filter(|v| counts[**v] > 1).copied())?;
        let submit = data::one(tokens.iter().filter(|v| **v != state).copied())?;
        let suffix = data::one(
            values
                .iter()
                .filter(|v| re(r"^[A-Za-z]{128}$").is_match(v) && counts[**v] == 2)
                .copied(),
        )?;
        let numbers: HashMap<_, _> = counts
            .iter()
            .filter(|(v, _)| re(r"^(?:0|[1-9][0-9]{0,3})$").is_match(v))
            .map(|(v, n)| (v.parse::<u64>().unwrap(), *n))
            .collect();
        let unique = |v: Vec<u64>| -> Result<u64> {
            let s: HashSet<_> = v.into_iter().collect();
            require(s.len() == 1, "Ambiguous work operand")?;
            Ok(*s.iter().next().unwrap())
        };
        let budget = unique(
            numbers
                .iter()
                .filter(|(n, c)| **c == 2 && **n > 0 && **n <= 1000)
                .map(|(n, _)| *n)
                .collect(),
        )?;
        let mode = unique(
            numbers
                .iter()
                .filter(|(n, c)| **c == 2 && **n != budget)
                .map(|(n, _)| *n)
                .collect(),
        )?;
        let index = unique(
            numbers
                .iter()
                .filter(|(n, c)| **c == 1 && (2..=129).contains(*n))
                .map(|(n, _)| *n)
                .collect(),
        )?;
        let input = data::json_input(&values)?;
        require(
            mode == 0 && !tokens.contains(input.as_str()),
            "Unsupported proof mode",
        )?;
        Ok(
            json!({"flow":self.one(data::FLOW)?,"hmwT6":self.one(data::TASK)?,"nuvy8":input,"nidg0":self.one(data::NONCE)?,"yVlbA2":submit,"vaZBm3":state,"suffix":suffix,"budgetMs":budget,"mode":mode,"previousCount":index}),
        )
    }
    pub fn precursor(&self) -> Result<Value> {
        let values = self.values();
        let names: HashSet<_> = self.literals.iter().map(|l| l.value.as_str()).collect();
        require(
            [
                "SlQq4", "QhCr7", "CyYT8", "hmwT6", "nuvy8", "vaZBm3", "yVlbA2",
            ]
            .iter()
            .all(|v| names.contains(v)),
            "Precursor declaration",
        )?;
        Ok(
            json!({"flow":self.one(data::FLOW)?,"hmwT6":self.one(data::TASK)?,"nuvy8":data::json_input(&values)?,"yVlbA2":data::one(values.iter().filter(|v|data::token(v)&&v.contains("-1.3.1.1-")).copied())?,"nidg0":self.one(data::NONCE)?,"vaZBm3":"","script":data::one(names.into_iter().filter(|v|re(r"^/scripts/precursor/[a-f0-9]{12}/main\.js$").is_match(v)))?}),
        )
    }
}
#[derive(Clone)]
pub struct Navigation {
    pub url: String,
    pub body: Vec<u8>,
}
fn form_fields(values: &[&str], start: usize) -> Result<([usize; 3], usize)> {
    let a = start + 8;
    let b = a + 3;
    let c = b + 3;
    let token = c + 1;
    let names = [a, b, c].map(|i| values.get(i).copied().unwrap_or(""));
    require(
        names
            .iter()
            .all(|name| re(r"^[a-f0-9]{64}$").is_match(name))
            && names.iter().collect::<HashSet<_>>().len() == 3
            && values.get(token).is_some_and(|v| {
                (100..=16384).contains(&v.len())
                    && re(r"^[A-Za-z0-9_.-]+-[0-9]{10}-1\.0\.1\.1-[A-Za-z0-9_.-]+$").is_match(v)
            }),
        "Parent form fields",
    )?;
    require(
        values[a - 2..a] == ["input", "hidden"]
            && values[a + 1..b] == ["input", "hidden"]
            && values[b + 1..c] == ["input", "hidden"],
        "Parent form order",
    )?;
    Ok(([a, b, c], token))
}
pub fn navigation(
    data: &Response,
    plan: &Plan,
    config: &Value,
    page_url: &str,
) -> Result<Option<Navigation>> {
    let values: Vec<_> = data
        .literals
        .iter()
        .map(|l| l.value.as_str())
        .filter(|s| !s.is_empty())
        .collect();
    if !values.contains(&"form") || !values.contains(&"POST") {
        return Ok(None);
    }
    require(
        matches!(
            plan.meta["build"].as_str(),
            Some("d2d3316b729e" | "c787ce48cf6b")
        ) && data.program.code_length <= 8192
            && !values.contains(&"fail"),
        "Unsupported parent navigation format",
    )?;
    let start = values.iter().position(|v| *v == "form").unwrap();
    let expected = [
        "form",
        "action",
        "method",
        "POST",
        "enctype",
        "application/x-www-form-urlencoded",
        "input",
        "hidden",
    ];
    require(
        values.get(start..start + expected.len()) == Some(expected.as_slice()),
        "Parent form declaration",
    )?;
    let ([a, b, c], token) = form_fields(&values, start)?;
    let before = validate_url(page_url)?;
    let target = before
        .join(config["fa"].as_str().unwrap_or(""))
        .map_err(|_| crate::Error::new("INVALID_DATA", "Form URL"))?;
    let query: Vec<_> = target
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    let issued: Vec<_> = query.iter().filter(|(k, _)| k == "__cf_chl_f_tk").collect();
    let ordinary = |url: &url::Url| {
        url.query_pairs()
            .filter(|(k, _)| !k.starts_with("__cf_chl_"))
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect::<Vec<_>>()
    };
    require(
        origin(target.as_str())? == origin(page_url)?
            && target.path() == before.path()
            && issued.len() == 1
            && re(r"^[A-Za-z0-9_.-]{20,512}$").is_match(&issued[0].1)
            && ordinary(&target) == ordinary(&before),
        "Form does not belong to current page",
    )?;
    let mut body = url::form_urlencoded::Serializer::new(String::new());
    body.append_pair(values[a], string(&config["md"])?);
    if let Some(v) = config["mdrd"].as_str().filter(|v| !v.is_empty()) {
        body.append_pair(values[b], v);
    }
    body.append_pair(values[c], values[token]);
    Ok(Some(Navigation {
        url: target.into(),
        body: body.finish().into_bytes(),
    }))
}
pub fn current_page_token(config: &Value) -> Result<String> {
    let url = validate_url("https://placeholder.invalid/")?
        .join(string(&config["cUPMDTk"])?)
        .map_err(|_| crate::Error::new("INVALID_DATA", "Current page token URL"))?;
    let tokens: Vec<_> = url
        .query_pairs()
        .filter(|(k, _)| k == "__cf_chl_tk")
        .map(|(_, v)| v.into_owned())
        .collect();
    require(tokens.len() == 1, "Missing current page token")?;
    Ok(tokens[0].clone())
}
pub enum Next {
    Done(Value),
    Work(Value),
}
pub fn next(response: &Response) -> Result<Next> {
    next_for(response, false)
}
pub fn next_for(response: &Response, embedded: bool) -> Result<Next> {
    if let Ok(done) = if embedded {
        response.widget_completion()
    } else {
        response.completion()
    } {
        return Ok(Next::Done(done));
    }
    require(
        !response.literals.iter().any(|l| l.value == "fail"),
        "Challenge task results were not accepted",
    )?;
    response.proof().map(Next::Work).map_err(|_| {
        crate::Error::new(
            "MANAGED_WORK_UNSUPPORTED",
            "Unrecognized follow-up declaration",
        )
    })
}
/// The current page's explicitly issued non-interactive completion navigation.
pub fn issued_url(page: &str, config: &serde_json::Value) -> crate::Result<String> {
    if config["cType"] != "non-interactive" || config["fa"].is_null() || config["fa"] == "" {
        return Ok(page.into());
    }
    let path = crate::string(&config["fa"])?;
    crate::require(
        path.starts_with('/') && !path.starts_with("//"),
        "Invalid issued completion path",
    )?;
    let before = crate::static_data::validate_url(page)?;
    let after = before
        .join(path)
        .map_err(|_| crate::Error::new("INVALID_DATA", "Completion URL"))?;
    let tokens = after
        .query_pairs()
        .filter(|(k, _)| k == "__cf_chl_f_tk")
        .map(|(_, v)| v.into_owned())
        .collect::<Vec<_>>();
    let ordinary = |u: &url::Url| {
        u.query_pairs()
            .filter(|(k, _)| !k.starts_with("__cf_chl_"))
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect::<Vec<_>>()
    };
    crate::require(
        before.origin() == after.origin()
            && before.path() == after.path()
            && ordinary(&before) == ordinary(&after)
            && tokens.len() == 1
            && crate::re(r"^[A-Za-z0-9_.-]{20,512}$").is_match(&tokens[0]),
        "Issued completion does not belong to this page",
    )?;
    Ok(after.into())
}
