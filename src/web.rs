use crate::{Call, Service, authorized, error_response, model};
use anyhow::{Result, ensure};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use serde::Deserialize;
use serde_json::{Value, json};

const SOURCE: &str = include_str!("../source/fanqie.js");
const SOURCE_CONFIG: &str = include_str!("../source/source.config.json");
fn asset(name: &str, content_type: &str, embedded: &str) -> Response {
    let body = std::env::var_os("FANQIE_RELAY_WEB_DIR")
        .and_then(|dir| std::fs::read_to_string(std::path::Path::new(&dir).join(name)).ok())
        .unwrap_or_else(|| embedded.into());
    (
        [
            (header::CONTENT_TYPE, content_type),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        body,
    )
        .into_response()
}

pub fn routes() -> Router<Service> {
    Router::new()
        .route(
            "/",
            get(|| async {
                asset(
                    "index.html",
                    "text/html; charset=utf-8",
                    include_str!("../web/index.html"),
                )
            }),
        )
        .route(
            "/app.css",
            get(|| async {
                asset(
                    "app.css",
                    "text/css; charset=utf-8",
                    include_str!("../web/app.css"),
                )
            }),
        )
        .route(
            "/app.js",
            get(|| async {
                asset(
                    "app.js",
                    "text/javascript; charset=utf-8",
                    include_str!("../web/app.js"),
                )
            }),
        )
        .route(
            "/favicon.svg",
            get(|| async {
                asset(
                    "favicon.svg",
                    "image/svg+xml",
                    include_str!("../web/favicon.svg"),
                )
            }),
        )
        .route("/source.js", get(source))
        .route(
            "/community",
            get(|| async {
                asset(
                    "community.html",
                    "text/html; charset=utf-8",
                    include_str!("../web/community.html"),
                )
            }),
        )
        .route(
            "/community.js",
            get(|| async {
                asset(
                    "community.js",
                    "text/javascript; charset=utf-8",
                    include_str!("../web/community.js"),
                )
            }),
        )
        .route("/source.json", get(source_json))
        .route("/qr.svg", get(qr))
        .route("/admin/status", get(status))
        .route("/admin/network", get(network_get))
        .route("/admin/catalog", get(catalog))
        .route("/admin/requests/{id}", get(detail))
}
pub fn public_url() -> Result<Option<String>> {
    std::env::var("FANQIE_RELAY_PUBLIC_URL")
        .ok()
        .filter(|base| !base.trim().is_empty())
        .map(|base| normalize_base(&base))
        .transpose()
}
fn first_header<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get(name)?
        .to_str()
        .ok()?
        .split(',')
        .next()
        .map(str::trim)
}
fn request_base(
    explicit: Option<&str>,
    configured: Option<&str>,
    headers: &HeaderMap,
) -> Result<String> {
    if let Some(base) = explicit.or(configured) {
        return normalize_base(base);
    }
    // Proxies should replace these headers with the original public host/protocol.
    let forwarded = first_header(headers, "forwarded").unwrap_or("");
    let parameter = |name: &str| {
        forwarded.split(';').find_map(|part| {
            let (key, value) = part.trim().split_once('=')?;
            key.trim()
                .eq_ignore_ascii_case(name)
                .then(|| value.trim().trim_matches('"'))
        })
    };
    let host = parameter("host")
        .or_else(|| first_header(headers, "x-forwarded-host"))
        .or_else(|| first_header(headers, "host"))
        .ok_or_else(|| anyhow::anyhow!("请求缺少服务主机名"))?;
    let scheme = parameter("proto")
        .or_else(|| first_header(headers, "x-forwarded-proto"))
        .unwrap_or("http");
    ensure!(
        matches!(scheme, "http" | "https"),
        "服务协议必须是 HTTP/HTTPS"
    );
    let _authority: axum::http::uri::Authority = host.parse()?;
    normalize_base(&format!("{scheme}://{host}"))
}
pub fn normalize_base(base: &str) -> Result<String> {
    let mut url = url::Url::parse(base.trim())?;
    ensure!(
        matches!(url.scheme(), "http" | "https")
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none(),
        "服务地址必须是 HTTP/HTTPS 地址"
    );
    ensure!(
        url.path() == "/" && url.query().is_none() && url.fragment().is_none(),
        "服务地址不应包含路径、查询或片段"
    );
    url.set_path("");
    Ok(url.as_str().trim_end_matches('/').into())
}
#[derive(Default, Deserialize)]
struct SourceQuery {
    base: Option<String>,
    download: Option<String>,
}
async fn qr(
    State(service): State<Service>,
    headers: HeaderMap,
    Query(q): Query<SourceQuery>,
) -> Response {
    if service.token.is_some() {
        return error_response(
            StatusCode::CONFLICT,
            "QR_IMPORT_UNAVAILABLE",
            "服务已启用令牌，请下载 JSON 书源后导入",
        );
    }
    let base = match request_base(q.base.as_deref(), service.public_url.as_deref(), &headers) {
        Ok(v) => v,
        Err(_) => return error_response(StatusCode::BAD_REQUEST, "INVALID_BASE", "服务地址无效"),
    };
    let mut import_url = url::Url::parse(&format!("{base}/source.json")).unwrap();
    import_url.query_pairs_mut().append_pair("base", &base);
    // Legado's book-source scanner imports HTTP URLs directly.
    let code = match qrcode::QrCode::new(import_url.as_str()) {
        Ok(c) => c,
        Err(_) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                "INVALID_BASE",
                "地址太长，无法生成二维码",
            );
        }
    };
    (
        [
            (header::CONTENT_TYPE, "image/svg+xml"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        code.render::<qrcode::render::svg::Color>()
            .min_dimensions(240, 240)
            .build(),
    )
        .into_response()
}
fn source_text(base: &str, token: Option<&str>) -> String {
    SOURCE
        .replace(
            "'http://127.0.0.1:19670/fanqie'",
            &serde_json::to_string(&format!("{base}/fanqie")).unwrap(),
        )
        .replace(
            "'http://127.0.0.1:19670'",
            &serde_json::to_string(base).unwrap(),
        )
        .replace(
            "var RELAY_TOKEN = '';",
            &format!(
                "var RELAY_TOKEN = {};",
                serde_json::to_string(token.unwrap_or("")).unwrap()
            ),
        )
}
async fn source(
    State(service): State<Service>,
    headers: HeaderMap,
    Query(q): Query<SourceQuery>,
) -> Response {
    if !authorized(&service, &headers) {
        return error_response(
            StatusCode::UNAUTHORIZED,
            "TOKEN_REQUIRED",
            "请先在管理页填写服务令牌，再下载书源文件导入阅读",
        );
    }
    let base = match request_base(q.base.as_deref(), service.public_url.as_deref(), &headers) {
        Ok(v) => v,
        Err(_) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                "INVALID_BASE",
                "请填写不包含路径的服务地址，例如 http://192.168.1.10:19670",
            );
        }
    };
    let mut r = (
        [
            (header::CONTENT_TYPE, "text/plain; charset=utf-8"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        source_text(&base, service.token.as_deref()),
    )
        .into_response();
    if q.download.as_deref() == Some("1") {
        r.headers_mut().insert(
            header::CONTENT_DISPOSITION,
            "attachment; filename=fanqie-rust.js".parse().unwrap(),
        );
    }
    r
}
async fn source_json(
    State(service): State<Service>,
    headers: HeaderMap,
    Query(q): Query<SourceQuery>,
) -> Response {
    if !authorized(&service, &headers) {
        return error_response(
            StatusCode::UNAUTHORIZED,
            "TOKEN_REQUIRED",
            "请先在管理页填写服务令牌，再下载书源文件导入阅读",
        );
    }
    let base = match request_base(q.base.as_deref(), service.public_url.as_deref(), &headers) {
        Ok(v) => v,
        Err(_) => return error_response(StatusCode::BAD_REQUEST, "INVALID_BASE", "服务地址无效"),
    };
    let mut config: Value = serde_json::from_str(SOURCE_CONFIG).unwrap();
    config["bookSourceUrl"] = json!(format!("{base}/fanqie"));
    config["mainJs"] = json!(source_text(&base, service.token.as_deref()));
    let mut response = (
        [
            (header::CONTENT_TYPE, "application/json; charset=utf-8"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        serde_json::to_string(&vec![config]).unwrap(),
    )
        .into_response();
    if q.download.as_deref() == Some("1") {
        response.headers_mut().insert(
            header::CONTENT_DISPOSITION,
            "attachment; filename=fanqie-rust.json".parse().unwrap(),
        );
    }
    response
}
async fn status(State(service): State<Service>, headers: HeaderMap) -> Response {
    if !authorized(&service, &headers) {
        return error_response(
            StatusCode::UNAUTHORIZED,
            "TOKEN_REQUIRED",
            "请填写服务令牌以查看管理信息",
        );
    }
    let mut value = service.monitor.snapshot();
    let public_url = match request_base(None, service.public_url.as_deref(), &headers) {
        Ok(v) => v,
        Err(_) => return error_response(StatusCode::BAD_REQUEST, "INVALID_BASE", "服务地址无效"),
    };
    value["service"] = json!({"version":env!("CARGO_PKG_VERSION"),"platform":std::env::consts::OS,"arch":std::env::consts::ARCH,"listen":service.listen,"publicUrl":public_url,"configuredPublicUrl":service.public_url,"responseCache":false,"tokenEnabled":service.token.is_some(),"concurrency":service.max_concurrency,"contentConcurrencyPerDevice":1,"contentBatchSize":30,"batchIntervalMs":service.batch_interval_ms,"contentQueueTimeoutMs":45000,"availableSlots":service.concurrency.available_permits(),"gzip":true,"memory":crate::runtime::memory()});
    value["service"]["network"] = service.network.status();
    value["tips"] = json!([
        {"level":"info","title":"每次刷新都取得最新数据","text":"实时请求官方接口，连接池与设备密钥复用不会缓存书籍内容。"},
        {"level":"info","title":"书源自动使用当前访问地址","text":"域名、端口和 HTTPS 跟随访问地址；127.0.0.1 在手机上指向手机自身。"},
        {"level":"info","title":"登录后可访问书架和历史","text":"导入后在书源登录界面完成官方网页登录，会话由阅读保存。"}
    ]);
    Json(value).into_response()
}
async fn network_get(State(service): State<Service>, headers: HeaderMap) -> Response {
    if !authorized(&service, &headers) {
        return error_response(
            StatusCode::UNAUTHORIZED,
            "TOKEN_REQUIRED",
            "请填写服务令牌以查看网络状态",
        );
    }
    Json(service.network.status()).into_response()
}
fn parse_exact(text: &str) -> Option<Value> {
    let mut v = serde_json::from_str::<Value>(text).ok()?;
    model::protect_ids(&mut v);
    crate::monitor::redact(&mut v);
    Some(v)
}
async fn detail(
    State(service): State<Service>,
    headers: HeaderMap,
    Path(id): Path<u64>,
) -> Response {
    if !authorized(&service, &headers) {
        return error_response(
            StatusCode::UNAUTHORIZED,
            "TOKEN_REQUIRED",
            "服务令牌未配置或不正确",
        );
    }
    let Some((record, d)) = service.monitor.detail(id) else {
        return error_response(
            StatusCode::NOT_FOUND,
            "DETAIL_EXPIRED",
            "此记录的详细数据已释放；请重新发起请求。概览仍保留最近 200 条记录。",
        );
    };
    let upstream:Vec<_>=d.upstream.iter().map(|t|json!({"path":t.path,"status":t.status,"body":t.body,"bytes":t.bytes,"credentialsHidden":t.credentials_hidden,"data":parse_exact(&t.body)})).collect();
    Json(json!({"record":record,"request":d.request,"response":d.response,"result":parse_exact(&d.response),"metrics":d.metrics,"upstream":upstream})).into_response()
}
pub fn call_summary(call: &Call) -> String {
    let args = &call.args;
    match call.operation.as_str() {
        "search" => format!(
            "{} · 第 {} 页",
            model::s(args, "key"),
            crate::api::number(&args["page"]).max(1)
        ),
        "explore" => format!(
            "{} · 第 {} 页",
            model::s(args, "url"),
            crate::api::number(&args["page"]).max(1)
        ),
        "detail" | "chapters" => model::book_id(&args["book"]).unwrap_or_default(),
        "content" => model::s(&args["chapter"], "url"),
        "content_batch" => format!(
            "{} · {} 章",
            model::book_id(&args["book"]).unwrap_or_default(),
            args["chapters"].as_array().map_or(0, Vec::len)
        ),
        "progress_get" | "progress_put" | "shelf_change" | "review_summary" | "reviews"
        | "review_replies" => format!(
            "{} · {}",
            model::book_id(&args["book"]).unwrap_or_default(),
            model::s(args, "scope")
        ),
        "discovery_menu" => "实时官方榜单、分类与书架分组".into(),
        "raw" => model::s(args, "path"),
        "web_login" => "官方网页登录验证".into(),
        _ => String::new(),
    }
    .chars()
    .take(160)
    .collect()
}
pub fn safe_error(err: &anyhow::Error) -> String {
    if let Some(e) = err.downcast_ref::<reqwest::Error>() {
        return if e.is_timeout() {
            "官方接口请求超时，请稍后重试"
        } else {
            "无法连接官方接口，请检查 Windows 网络或代理配置"
        }
        .into();
    }
    let text = err.to_string();
    if text.starts_with("番茄")
        || text.starts_with("官方")
        || text.starts_with("请先")
        || text.starts_with("书籍")
        || text.starts_with("章节")
    {
        return text.chars().take(180).collect();
    }
    "接口处理失败，请检查参数或账号会话，并查看原始 API 返回值".into()
}
pub fn allowed_path(path: &str) -> bool {
    matches!(
        path,
        "/reading/bookapi/detail/v"
            | "/reading/bookapi/directory/all_items/v"
            | "/reading/bookapi/multi-detail/v"
            | "/reading/bookapi/search/search/v"
            | "/reading/bookapi/bookmall/homepage/v"
            | "/reading/bookapi/bookmall/cell/change/v"
            | "/reading/bookapi/bookshelf/list/v"
            | "/reading/bookapi/read_history/list/v"
            | "/reading/reader/full/v"
            | "/reading/reader/batch_full/v"
            | "/reading/bookapi/new_category/front/v"
            | "/reading/bookapi/new_category/landing/v"
            | "/reading/bookapi/bookmall/cell/change/v1"
            | "/reading/bookapi/bookmall/tab/v"
            | "/reading/ugc/idea/list/v"
            | "/reading/ugc/postdata/comment/v"
            | "/reading/ugc/reply/item_detail/v"
    ) || allowed_post_path(path)
}
pub fn allowed_post_path(path: &str) -> bool {
    path == "/reading/ugc/item/mix_data/get/v"
        || regex::Regex::new(r"^/novel/commentapi/(?:comment|reply)/list/[0-9]{1,20}/v1$")
            .unwrap()
            .is_match(path)
}
async fn catalog(State(service): State<Service>, headers: HeaderMap) -> Response {
    if !authorized(&service, &headers) {
        return error_response(
            StatusCode::UNAUTHORIZED,
            "TOKEN_REQUIRED",
            "服务令牌未配置或不正确",
        );
    }
    Json(json!({"operations":["search","detail","chapters","content","content_batch","progress_get","progress_put","explore","discovery_menu","review_summary","reviews","review_replies","shelf_change","raw","web_login"],"paths":[
        {"path":"/reading/bookapi/new_category/front/v","name":"实时男生 / 女生分类与标签","operation":"discovery_menu","params":{}},
        {"path":"/reading/bookapi/new_category/landing/v","name":"分类与字数 / 状态 / 排序筛选","operation":"explore","params":{}},
        {"path":"/reading/bookapi/bookmall/cell/change/v1","name":"官方榜单（名称及编号从实时导航读取）","operation":"explore","params":{}},
        {"path":"/reading/ugc/idea/list/v","name":"原生段评数量与版本","operation":"review_summary","params":{}},
        {"path":"/novel/commentapi/comment/list/{id}/v1","name":"书评与段评（只读）","operation":"reviews","method":"POST","params":{}},
        {"path":"/reading/ugc/item/mix_data/get/v","name":"章节讨论（只读）","operation":"reviews","method":"POST","params":{}},
        {"path":"/novel/commentapi/reply/list/{id}/v1","name":"评论回复（只读）","operation":"review_replies","method":"POST","params":{}},
        {"path":"/reading/bookapi/bookshelf/add|delete/v","name":"云端书架增删（默认预检查）","operation":"shelf_change","method":"POST","params":{}},
        {"path":"/reading/bookapi/search/search/v","name":"搜索","params":{"q":"烟雨楼","offset":"0"}},
        {"path":"/reading/bookapi/detail/v","name":"书籍详情","params":{"book_id":"6883748331202284558"}},
        {"path":"/reading/bookapi/directory/all_items/v","name":"完整目录","params":{"book_id":"6883748331202284558","filter_copyright_page":"false"}},
        {"path":"/reading/bookapi/multi-detail/v","name":"批量详情","params":{"book_id":"6883748331202284558","book_type":"0"}},
        {"path":"/reading/bookapi/bookmall/homepage/v","name":"旧首页栏目（含高分佳作）","params":{}},
        {"path":"/reading/bookapi/bookmall/tab/v","name":"推荐频道（连续推荐流）","params":{"tab_type":"2","client_fetch_unlimited_mode":"1"}},
        {"path":"/reading/bookapi/bookmall/cell/change/v","name":"栏目落地页 / 高分换批","params":{"cell_id":"","show_type":"337","offset":"0","limit":"20","change_type":"1"}},
        {"path":"/reading/bookapi/bookshelf/list/v","name":"账号书架","params":{"server_time":"0"}},
        {"path":"/reading/bookapi/read_history/list/v","name":"阅读历史","params":{"book_type":"0","offset":"0","limit":"20","full_field":"true","is_first_load":"true"}},
        {"path":"/reading/reader/batch_full/v","name":"批量正文（原版下载模式，最多 30 章）","operation":"content_batch","method":"GET","params":{}},
        {"path":"/reading/bookapi/read_progress/get/v","name":"读取账号章节进度","operation":"progress_get","method":"POST","params":{}},
        {"path":"/reading/bookapi/read_progress/upload/v","name":"上传章节与阅读历史","operation":"progress_put","method":"POST","params":{}},
        {"path":"/reading/reader/full/v","name":"章节 API（含加密字段）","params":{"book_id":"6883748331202284558","item_id":"6883749008234250760","novel_text_type":"1"}}
    ],"metadata":{"book":"variable.fanqie 保存完整书籍字段；书架与历史额外保存 reading 上下文。","chapter":"variable.fanqie 保存每章完整字段；字数、发布/修改时间、VIP/购买标志和阅读标记映射为阅读支持的字段。","body":"官方正文的加密、压缩和段落等字段可在请求详情中查看；返回给阅读的正文已解密。","ids":"16 位及以上整数在字段树中转为字符串，原始 JSON 文本保留原值。"}})).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn import_address_follows_host_port_and_proxy_https() {
        let cases: &[(&[(&str, &str)], &str)] = &[
            (&[("host", "relay.lan:122")], "http://relay.lan:122"),
            (&[("host", "1.1.1.1:8088")], "http://1.1.1.1:8088"),
            (&[("host", "[::1]:122")], "http://[::1]:122"),
            (
                &[
                    ("host", "container:19670"),
                    ("x-forwarded-host", "books.example:8443, proxy.local"),
                    ("x-forwarded-proto", "https, http"),
                ],
                "https://books.example:8443",
            ),
            (
                &[
                    ("host", "container:19670"),
                    (
                        "forwarded",
                        "for=192.0.2.1; Host=\"books.example:443\"; Proto=https, for=proxy",
                    ),
                    ("x-forwarded-host", "ignored.example"),
                ],
                "https://books.example",
            ),
        ];
        for (values, expected) in cases {
            let mut headers = HeaderMap::new();
            for (name, value) in *values {
                headers.insert(
                    header::HeaderName::from_bytes(name.as_bytes()).unwrap(),
                    value.parse().unwrap(),
                );
            }
            let base = request_base(None, None, &headers).unwrap();
            assert_eq!(&base, expected);
            assert!(source_text(&base, None).contains(&serde_json::to_string(&base).unwrap()));
        }
    }
    #[test]
    fn import_address_overrides_and_invalid_authorities() {
        let mut headers = HeaderMap::new();
        headers.insert(header::HOST, "relay.lan:122".parse().unwrap());
        assert_eq!(
            request_base(None, Some("https://fixed.example"), &headers).unwrap(),
            "https://fixed.example"
        );
        assert_eq!(
            request_base(
                Some("http://manual.lan:8088/"),
                Some("https://fixed.example"),
                &headers
            )
            .unwrap(),
            "http://manual.lan:8088"
        );
        for host in [
            "user@relay.lan",
            "relay.lan/path",
            "relay.lan?query",
            "relay.lan:badport",
        ] {
            headers.insert(header::HOST, host.parse().unwrap());
            assert!(request_base(None, None, &headers).is_err(), "{host}");
        }
        headers.insert(header::HOST, "relay.lan:122".parse().unwrap());
        headers.insert("x-forwarded-proto", "file".parse().unwrap());
        assert!(request_base(None, None, &headers).is_err());
        assert!(request_base(None, None, &HeaderMap::new()).is_err());
    }
    #[test]
    fn exported_source_quotes_host_and_token_and_preserves_exact_ids() {
        let base = normalize_base("http://my-pc.lan:19670/").unwrap();
        let source = source_text(&base, Some("test'\"token"));
        assert!(source.contains("\"http://my-pc.lan:19670/fanqie\""));
        assert!(source.contains("var RELAY_TOKEN = \"test'\\\"token\";"));
        assert!(normalize_base("file:///etc/passwd").is_err());
        assert!(normalize_base("http://host/path").is_err());
        assert_eq!(
            parse_exact("{\"id\":6883749008234250760}").unwrap()["id"],
            "6883749008234250760"
        );
        let config: Value = serde_json::from_str(SOURCE_CONFIG).unwrap();
        assert_eq!(config["eventListener"], true);
        assert_eq!(config["ruleContent"]["maxBatchSize"], 30);
        assert!(
            config["ruleContent"]["callBackJs"]
                .as_str()
                .unwrap()
                .contains("relayOnEvent")
        );
    }
}
