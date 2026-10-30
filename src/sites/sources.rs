//! Embedded example sources. No generated source files are needed at runtime.
use crate::{epoch_ms, syntax::static_data::validate_url, Error, Result};
use serde_json::{json, Value};

pub fn render(site: &str, base: &str, token: &str) -> Result<String> {
    generate(site, base, token).map(|(_, script)| script)
}

pub fn bundle(base: &str, token: &str) -> Result<Value> {
    let mut sources = Vec::new();
    for site in ["twkan", "69shuba", "banxia"] {
        let (mut config, script) = generate(site, base, token)?;
        config["mainJs"] = json!(script);
        sources.push(config);
    }
    Ok(json!(sources))
}

fn generate(site: &str, base: &str, token: &str) -> Result<(Value, String)> {
    let supplied = url::Url::parse(base).map_err(|_| Error::new("INVALID_BASE", "服务地址无效"))?;
    validate_url(base)?;
    if supplied.query().is_some() || supplied.fragment().is_some() {
        let mut e = Error::new("INVALID_BASE", "服务地址不能带查询参数或片段");
        e.status = 400;
        return Err(e);
    }
    let (name, origin, parser) = match site {
        "twkan" => (
            "台湾小说网",
            "https://twkan.com",
            include_str!("../../assets/sources/twkan-parser.js"),
        ),
        "69shuba" => (
            "69书吧",
            "https://www.69shuba.com",
            include_str!("../../assets/sources/69shuba-parser.js"),
        ),
        "banxia" => (
            "半夏小说",
            "https://www.xbanxia.cc",
            include_str!("../../assets/sources/banxia-parser.js"),
        ),
        _ => {
            let mut e = Error::new("INVALID_SITE", "未知接入示例");
            e.status = 400;
            return Err(e);
        }
    };
    let comment = if site == "69shuba" {
        "支持关键词搜索与完整书籍链接检索。GBK 搜索表单及嵌入式验证由服务处理。"
    } else {
        "搜索与输出使用阅读内置简繁转换。"
    };
    let config = json!({"bookSourceUrl":format!("{origin}/cf-gateway-relay"),"bookSourceName":format!("{name}（CF 代取）"),"bookSourceGroup":"CF Gateway","bookSourceType":0,"enabled":true,"enabledExplore":true,"enabledCookieJar":false,"exploreUrl":"@js:eval(String(source.mainJs)); JSON.stringify(cfDiscoveryLayout(cfDiscovery()));","bookSourceComment":format!("CF Gateway 页面代取示例。{comment}页面不缓存。服务地址：{}",base.trim_end_matches('/')),"lastUpdateTime":epoch_ms()});
    let mut output = format!("var config = {};\n", serde_json::to_string_pretty(&config)?);
    for (key, value) in [
        ("CF_SITE_ID", site),
        ("CF_SITE", origin),
        ("CF_BASE", base.trim_end_matches('/')),
        ("CF_TOKEN", token),
    ] {
        output.push_str(&format!("var {key} = {};\n", serde_json::to_string(value)?));
    }
    output.push_str(include_str!("../../assets/sources/gateway-source.js"));
    output.push_str(parser);
    Ok((config, output))
}
