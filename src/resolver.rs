use crate::{api::Api, model};
use anyhow::{Result, anyhow, ensure};
use std::time::{Duration, Instant};

pub fn share_input(input: &str) -> Option<String> {
    let re = regex::Regex::new(r#"https?://[A-Za-z0-9_./?&=%#:+~!$()*,-]+"#).unwrap();
    let raw = re.find(input)?.as_str().trim_end_matches([',', ')']);
    let u = url::Url::parse(raw).ok()?;
    (matches!(u.scheme(), "http" | "https")
        && u.username().is_empty()
        && u.password().is_none()
        && model::official_host(u.host_str()?))
    .then(|| u.to_string())
}
pub async fn resolve(api: &mut Api, input: &str) -> Result<String> {
    if let Some(id) = model::input_book_id(input) {
        return Ok(id);
    }
    let mut target = share_input(input)
        .ok_or_else(|| anyhow!("书籍分享地址无效，请使用番茄官方地址或 id:书籍ID"))?;
    if let Some(id) = model::input_book_id(&target) {
        return Ok(id);
    }
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(20))
        .build()?;
    for _ in 0..5 {
        let u = url::Url::parse(&target)?;
        ensure!(
            model::official_host(u.host_str().unwrap_or(""))
                && matches!(u.scheme(), "http" | "https"),
            "官方分享链接跳转地址无效"
        );
        let begin = Instant::now();
        let mut response = client.get(&target).send().await?;
        let status = response.status();
        let protocol = format!("{:?}", response.version());
        let location = response
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        let mut bytes = vec![];
        while let Some(chunk) = response.chunk().await? {
            ensure!(bytes.len() + chunk.len() <= 2 * 1024 * 1024, "分享页面过大");
            bytes.extend_from_slice(&chunk);
        }
        let html = String::from_utf8_lossy(&bytes).into_owned();
        api.record("/web/resolve-share", status.as_u16(), begin, protocol);
        api.capture("/web/resolve-share", status.as_u16(), &html);
        if status.is_redirection() {
            target = u
                .join(&location.ok_or_else(|| anyhow!("分享链接缺少跳转地址"))?)?
                .to_string();
            if let Some(id) = model::input_book_id(&target) {
                return Ok(id);
            }
            continue;
        }
        ensure!(status.is_success(), "分享页面 HTTP {}", status.as_u16());
        let re =
            regex::Regex::new(r#"["'](?:bookId|book_id)["']\s*:\s*["']?([0-9]{1,20})"#).unwrap();
        if let Some(id) = re
            .captures(&html)
            .and_then(|m| m.get(1))
            .and_then(|s| model::input_book_id(s.as_str()))
        {
            return Ok(id);
        }
        return Err(anyhow!("分享页面没有有效书籍 ID，请使用 id:书籍ID"));
    }
    Err(anyhow!("分享链接跳转次数过多"))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn canonical_ids_and_queries_stay_exact() {
        assert_eq!(
            model::input_book_id("id:6883748331202284558").as_deref(),
            Some("6883748331202284558")
        );
        assert_eq!(
            model::input_book_id(
                "https://reading.snssdk.com/reading/bookapi/detail/v/?book_id=6883748331202284558"
            )
            .as_deref(),
            Some("6883748331202284558")
        );
        assert!(model::input_book_id("https://evil.test/page/6883748331202284558").is_none());
        assert!(share_input("https://fanqienovel.com.evil.test/t/x").is_none());
        assert!(share_input("https://user:password@fanqienovel.com/t/x").is_none());
        assert!(share_input("分享 https://changdunovel.com/t/ABC/ 复制链接").is_some());
    }
}
