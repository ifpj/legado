use crate::{service::Service, Error, Result};
use axum::{
    extract::{DefaultBodyLimit, Query, State},
    http::{header, HeaderMap, StatusCode},
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
        include_str!("../assets/index.html")
            .replace("{{VERSION}}", crate::version::VERSION)
            .replace("{{BUILD_ID}}", crate::version::build_id()),
        "text/html; charset=utf-8",
    );
    response.headers_mut().insert("content-security-policy","default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; frame-src 'self' blob:; connect-src 'self'; base-uri 'none'; form-action 'self'; frame-ancestors 'none'".parse().unwrap());
    response
}
async fn css() -> Response {
    send(
        StatusCode::OK,
        include_str!("../assets/app.css").into(),
        "text/css; charset=utf-8",
    )
}
async fn js() -> Response {
    send(
        StatusCode::OK,
        include_str!("../assets/app.js").into(),
        "application/javascript; charset=utf-8",
    )
}
async fn logo() -> Response {
    let mut response = include_bytes!("../assets/logo.png")
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
    Json(options): Json<Value>,
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
    Json(options): Json<Value>,
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
    Json(options): Json<Value>,
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
        if let Ok(v) = value["timings"][key].to_string().parse() {
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
        value["imageType"].as_str().unwrap().parse().unwrap(),
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
    crate::sources::render("twkan", base, token)
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
    let script = crate::sources::render(
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
    let sources = crate::sources::bundle(
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
        .layer(DefaultBodyLimit::max(1024 * 1024))
        .with_state(state)
}
pub async fn serve(service: Service, host: &str, port: u16, token: String) -> Result<()> {
    let routes = router(service, token);
    let listener = tokio::net::TcpListener::bind((host, port)).await?;
    println!(
        "{}",
        json!({"service":"cf-gateway-rust","listen":format!("http://{host}:{port}"),"runtime":"Rust"})
    );
    axum::serve(listener, routes).await.map_err(Error::from)
}
