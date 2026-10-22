//! Embedded example sources. No generated source files are needed at runtime.
use crate::{epoch_ms, static_data::validate_url, Error, Result};
use serde_json::json;

pub fn render(site: &str, base: &str, token: &str) -> Result<String> {
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
            include_str!("../assets/twkan-parser.js"),
        ),
        "69shuba" => (
            "69书吧",
            "https://www.69shuba.com",
            include_str!("../assets/69shuba-parser.js"),
        ),
        "banxia" => (
            "半夏小说",
            "https://www.xbanxia.cc",
            include_str!("../assets/banxia-parser.js"),
        ),
        _ => {
            let mut e = Error::new("INVALID_SITE", "未知接入示例");
            e.status = 400;
            return Err(e);
        }
    };
    let comment = if site == "69shuba" {
        "69书吧站内搜索另需独立嵌入式验证。此示例支持发现、完整书籍链接检索、目录与正文；不提供关键词搜索。"
    } else {
        "搜索与输出使用阅读内置简繁转换。"
    };
    let config = json!({"bookSourceUrl":format!("{origin}/cf-gateway-relay"),"bookSourceName":format!("{name}（CF 代取）"),"bookSourceGroup":"CF Gateway","bookSourceType":0,"enabled":true,"enabledExplore":true,"enabledCookieJar":false,"exploreUrl":"@js:eval(String(source.mainJs)); JSON.stringify(cfDiscovery());","bookSourceComment":format!("CF Gateway 页面代取示例。{comment}页面不缓存。服务地址：{}",base.trim_end_matches('/')),"lastUpdateTime":epoch_ms()});
    let mut output = format!("var config = {};\n", serde_json::to_string_pretty(&config)?);
    for (key, value) in [
        ("CF_SITE_ID", site),
        ("CF_SITE", origin),
        ("CF_BASE", base.trim_end_matches('/')),
        ("CF_TOKEN", token),
    ] {
        output.push_str(&format!("var {key} = {};\n", serde_json::to_string(value)?));
    }
    output.push_str(include_str!("../assets/gateway-source.js"));
    output.push_str(parser);
    Ok(output)
}
