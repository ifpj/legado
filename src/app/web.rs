use crate::{app::service::Service, Error, Result};
use axum::{
    extract::{rejection::JsonRejection, DefaultBodyLimit, FromRequest, Query, Request, State},
    http::{header, HeaderMap, HeaderValue, Method, StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde_json::{json, Value};
use std::{collections::HashMap, sync::Arc};
#[derive(Clone)]
struct App {
    service: Arc<Service>,
    token: Arc<String>,
}
impl IntoResponse for Error {
    fn into_response(self) -> Response {
        (
            StatusCode::from_u16(self.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
            Json(self.reply()),
        )
            .into_response()
    }
}
/// A JSON request body. Framework rejections (syntax, media type, size) use
/// the same `{"ok":false,"code":…}` envelope as every other error.
struct Body(Value);
impl<S: Send + Sync> FromRequest<S> for Body {
    type Rejection = Error;
    async fn from_request(request: Request, state: &S) -> Result<Self> {
        match Json::<Value>::from_request(request, state).await {
            Ok(Json(value)) => Ok(Self(value)),
            Err(rejection) => Err(rejected(rejection)),
        }
    }
}
fn rejected(rejection: JsonRejection) -> Error {
    let (code, message) = match &rejection {
        JsonRejection::MissingJsonContentType(_) => {
            ("INVALID_REQUEST", "请求需要 Content-Type: application/json")
        }
        JsonRejection::BytesRejection(_) => ("REQUEST_TOO_LARGE", "请求体超过 1 MiB"),
        _ => ("INVALID_REQUEST", "请求体不是有效的 JSON"),
    };
    let mut error = Error::new(code, message);
    error.status = rejection.status().as_u16();
    error.details = json!({"reason":rejection.body_text()});
    error
}
async fn not_found(uri: Uri) -> Error {
    let mut error = Error::new("NOT_FOUND", "没有这个接口");
    error.status = 404;
    error.details = json!({"path":uri.path()});
    error
}
async fn method_not_allowed(method: Method, uri: Uri) -> Error {
    let mut error = Error::new("METHOD_NOT_ALLOWED", "此接口不支持该请求方法");
    error.status = 405;
    error.details = json!({"method":method.as_str(),"path":uri.path()});
    error
}
fn authorized(app: &App, headers: &HeaderMap) -> Result<()> {
    if app.token.is_empty() {
        return Ok(());
    }
    let expected = format!("Bearer {}", app.token);
    let actual = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let mut difference = expected.len() ^ actual.len();
    for (a, b) in expected.bytes().zip(actual.bytes()) {
        difference |= (a ^ b) as usize
    }
    if difference == 0 {
        Ok(())
    } else {
        let mut e = Error::new("UNAUTHORIZED", "需要服务访问令牌");
        e.status = 401;
        Err(e)
    }
}
fn authorized_readonly_link(
    app: &App,
    headers: &HeaderMap,
    query: &HashMap<String, String>,
) -> Result<()> {
    let mut supplied = headers.clone();
    if !supplied.contains_key(header::AUTHORIZATION) {
        if let Some(value) = query
            .get("token")
            .and_then(|t| format!("Bearer {t}").parse().ok())
        {
            supplied.insert(header::AUTHORIZATION, value);
        }
    }
    authorized(app, &supplied)
}
fn send(status: StatusCode, body: String, kind: &str) -> Response {
    send_bytes(status, body.into_bytes(), kind)
}
fn send_bytes(status: StatusCode, body: Vec<u8>, kind: &str) -> Response {
    let mut response = (status, body).into_response();
    let h = response.headers_mut();
    h.insert(header::CONTENT_TYPE, kind.parse().unwrap());
    h.insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    h.insert("x-content-type-options", "nosniff".parse().unwrap());
    h.insert("referrer-policy", "no-referrer".parse().unwrap());
    response
}
async fn index() -> Response {
    let mut response = send(
        StatusCode::OK,
        include_str!("../../assets/web/index.html").replace("{{BUILD}}", crate::version::build()),
        "text/html; charset=utf-8",
    );
    response.headers_mut().insert("content-security-policy","default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; frame-src 'self' blob:; connect-src 'self'; base-uri 'none'; form-action 'self'; frame-ancestors 'none'".parse().unwrap());
    response
}
async fn css() -> Response {
    send(
        StatusCode::OK,
        include_str!("../../assets/web/app.css").into(),
        "text/css; charset=utf-8",
    )
}
async fn js() -> Response {
    send(
        StatusCode::OK,
        include_str!("../../assets/web/app.js").into(),
        "application/javascript; charset=utf-8",
    )
}
async fn logo() -> Response {
    let mut response = include_bytes!("../../assets/web/logo.png")
        .as_slice()
        .into_response();
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, "image/png".parse().unwrap());
    response
}
async fn history(State(app): State<App>, headers: HeaderMap) -> Result<Json<Value>> {
    authorized(&app, &headers)?;
    Ok(Json(app.service.history()))
}
async fn status(State(app): State<App>, headers: HeaderMap) -> Result<Json<Value>> {
    authorized(&app, &headers)?;
    tokio::task::spawn_blocking(move || Json(app.service.status()))
        .await
        .map_err(|e| Error::new("TASK_ERROR", e.to_string()))
}
async fn fetch_json(
    State(app): State<App>,
    headers: HeaderMap,
    Body(options): Body,
) -> Result<Response> {
    authorized(&app, &headers)?;
    Ok(send_bytes(
        StatusCode::OK,
        app.service.fetch_json(options, false, false).await?,
        "application/json",
    ))
}
async fn solve(
    State(app): State<App>,
    headers: HeaderMap,
    Body(options): Body,
) -> Result<Response> {
    authorized(&app, &headers)?;
    Ok(send_bytes(
        StatusCode::OK,
        app.service.fetch_json(options, false, true).await?,
        "application/json",
    ))
}
async fn analyze(
    State(app): State<App>,
    headers: HeaderMap,
    Body(options): Body,
) -> Result<Response> {
    authorized(&app, &headers)?;
    Ok(send_bytes(
        StatusCode::OK,
        app.service.fetch_json(options, true, false).await?,
        "application/json",
    ))
}
async fn fetch_html(
    State(app): State<App>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Response> {
    authorized(&app, &headers)?;
    let mut options = serde_json::to_value(query)?;
    if let Some(value) = options["timeoutMs"].as_str() {
        options["timeoutMs"] = value
            .parse::<f64>()
            .ok()
            .and_then(serde_json::Number::from_f64)
            .map(Value::Number)
            .unwrap_or(Value::Null);
    }
    let (value, bytes) = app.service.html(options).await?;
    let mut response = send_bytes(
        StatusCode::from_u16(value["status"].as_u64().unwrap_or(502) as u16)
            .unwrap_or(StatusCode::BAD_GATEWAY),
        bytes,
        "text/html; charset=utf-8",
    );
    for (key, name) in [
        ("totalMs", "x-cf-gateway-ms"),
        ("queueMs", "x-cf-gateway-queue-ms"),
    ] {
        if let Some(v) = value["timings"][key]
            .as_f64()
            .and_then(|v| HeaderValue::from_str(&v.to_string()).ok())
        {
            response.headers_mut().insert(name, v);
        }
    }
    if let Some(kind) = value["challenge"]["type"].as_str() {
        response
            .headers_mut()
            .insert("x-cf-gateway-challenge", kind.parse().unwrap());
    }
    Ok(response)
}
async fn fetch_image(
    State(app): State<App>,
    headers: HeaderMap,
    Query(mut query): Query<HashMap<String, String>>,
) -> Result<Response> {
    authorized_readonly_link(&app, &headers, &query)?;
    query.remove("token");
    let mut options = serde_json::to_value(query)?;
    if let Some(timeout) = options["timeoutMs"].as_str() {
        options["timeoutMs"] = timeout
            .parse::<u64>()
            .map(Value::from)
            .unwrap_or(Value::Null);
    }
    let (value, bytes) = app.service.image(options).await?;
    let mut response = bytes.into_response();
    let h = response.headers_mut();
    h.insert(
        header::CONTENT_TYPE,
        value["imageType"]
            .as_str()
            .and_then(|kind| HeaderValue::from_str(kind).ok())
            .unwrap_or(HeaderValue::from_static("application/octet-stream")),
    );
    h.insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    h.insert("x-content-type-options", "nosniff".parse().unwrap());
    h.insert("referrer-policy", "no-referrer".parse().unwrap());
    h.insert(
        "content-security-policy",
        "default-src 'none'; sandbox".parse().unwrap(),
    );
    Ok(response)
}
pub fn source(base: &str, token: &str) -> Result<String> {
    crate::sites::sources::render("twkan", base, token)
}
async fn source_js(
    State(app): State<App>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Response> {
    // Source import and image loading cannot attach custom headers.
    authorized_readonly_link(&app, &headers, &query)?;
    let fallback = format!(
        "http://{}",
        headers
            .get(header::HOST)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("127.0.0.1:19686")
    );
    let site = query.get("site").map(String::as_str).unwrap_or("twkan");
    let script = crate::sites::sources::render(
        site,
        query.get("base").map(String::as_str).unwrap_or(&fallback),
        &app.token,
    )?;
    let mut response = send(
        StatusCode::OK,
        script,
        "application/javascript; charset=utf-8",
    );
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        format!("attachment; filename={site}-cf-relay.js")
            .parse()
            .unwrap(),
    );
    Ok(response)
}
async fn sources_json(
    State(app): State<App>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Response> {
    authorized_readonly_link(&app, &headers, &query)?;
    let fallback = format!(
        "http://{}",
        headers
            .get(header::HOST)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("127.0.0.1:19686")
    );
    let sources = crate::sites::sources::bundle(
        query.get("base").map(String::as_str).unwrap_or(&fallback),
        &app.token,
    )?;
    let mut response = send(
        StatusCode::OK,
        serde_json::to_string(&sources)?,
        "application/json; charset=utf-8",
    );
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        "attachment; filename=cf-gateway-sources.json"
            .parse()
            .unwrap(),
    );
    Ok(response)
}
pub fn router(service: Service, token: String) -> Router {
    let state = App {
        service: Arc::new(service),
        token: Arc::new(token),
    };
    Router::new()
        .route("/", get(index))
        .route("/app.css", get(css))
        .route("/app.js", get(js))
        .route("/logo.png", get(logo))
        .route("/v1/status", get(status))
        .route("/v1/history", get(history))
        .route("/source.js", get(source_js))
        .route("/sources.json", get(sources_json))
        .route("/fetch", get(fetch_html))
        .route("/image", get(fetch_image))
        .route("/v1/fetch", post(fetch_json))
        .route("/v1/solve", post(solve))
        .route("/v1/analyze", post(analyze))
        .fallback(not_found)
        .method_not_allowed_fallback(method_not_allowed)
        .layer(DefaultBodyLimit::max(1024 * 1024))
        .with_state(state)
}
/// Ctrl+C, or SIGTERM from `docker stop`, stops accepting and lets
/// in-flight requests finish within their own deadlines.
async fn shutdown_signal() {
    let interrupt = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        _ = interrupt => {}
        _ = terminate => {}
    }
    println!(
        "{}",
        json!({"service":"cf-gateway-rust","event":"shutdown","reason":"signal"})
    );
}
pub async fn serve(service: Service, host: &str, port: u16, token: String) -> Result<()> {
    let routes = router(service, token);
    let listener = tokio::net::TcpListener::bind((host, port)).await?;
    println!(
        "{}",
        json!({"service":"cf-gateway-rust","listen":format!("http://{host}:{port}"),"runtime":"Rust"})
    );
    axum::serve(listener, routes)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .map_err(Error::from)
}
