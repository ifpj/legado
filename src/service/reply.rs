use super::{credentials, image_type, total_deadline_error};
use crate::{engine::Outcome, network::Session, network::USER_AGENT, Result};
use serde_json::{json, Value};
use std::{sync::Arc, time::Instant};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Format {
    Value,
    Json,
    Html,
    Image,
}

pub(super) struct Reply {
    pub value: Value,
    pub bytes: Vec<u8>,
    pub upstream_bytes: usize,
}

pub(super) async fn prepare(
    outcome: Outcome,
    session: Arc<Session>,
    format: Format,
    analyze: bool,
    export: bool,
    queue_ms: f64,
    start: Instant,
    deadline: Instant,
) -> Result<Reply> {
    let mut reply = crate::cpu::run(deadline, move || {
        let image_kind = if format == Format::Image {
            Some(image_type(&outcome.response)?)
        } else {
            None
        };
        let Outcome { response, details, solved } = outcome;
        let upstream_bytes = response.content.len();
        let exported = if export {
            Some(credentials(&session, &response.url)?)
        } else {
            None
        };
        let mut value = json!({"ok":true,"status":response.status,"url":response.url,"headers":response.headers,"userAgent":USER_AGENT,"challenge":details,"contentCached":false,"precomputedEnvironmentProfile":false,"strictPureAlgorithmAcceptance":solved,"analyzed":analyze});
        let mut bytes = Vec::new();
        if let Some(kind) = image_kind {
            bytes = response.content;
            value["imageType"] = json!(kind);
            value["bytes"] = json!(bytes.len());
            drop(response.text);
        } else {
            drop(response.content);
            if let Some(exported) = exported {
                value["credentials"] = exported;
                drop(response.text);
            } else if format == Format::Html {
                bytes = response.text.into_text().into_bytes();
            } else {
                value["body"] = Value::String(response.text.into_text());
            }
        }
        if format == Format::Json {
            // Serialize the large body once, on a CPU worker. Append the small
            // timings field afterwards so this work is included in totalMs.
            serde_json::to_writer(&mut ResponseBuffer(&mut bytes), &value)?;
            let closing_brace = bytes.pop();
            debug_assert_eq!(closing_brace, Some(b'}'));
            value.as_object_mut().unwrap().remove("body");
            // The small suffix must not reallocate/copy the entire JSON body
            // later on the request thread. Two finite f64 values fit here.
            bytes.reserve(256);
        }
        Ok(Reply { value, bytes, upstream_bytes })
    })
    .await?;
    let timings = json!({"queueMs":queue_ms,"totalMs":start.elapsed().as_secs_f64() * 1000.});
    if format == Format::Json {
        reply.bytes.extend_from_slice(b",\"timings\":");
        serde_json::to_writer(&mut reply.bytes, &timings)?;
        reply.bytes.push(b'}');
    }
    reply.value["timings"] = timings;
    if Instant::now() >= deadline {
        return Err(total_deadline_error(&json!("preparingResponse"), None));
    }
    Ok(reply)
}

/// Bound copies and check cancellation while serializing a large JSON body.
struct ResponseBuffer<'a>(&'a mut Vec<u8>);
impl std::io::Write for ResponseBuffer<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        for chunk in bytes.chunks(64 * 1024) {
            crate::cpu::check().map_err(|e| std::io::Error::other(e.message))?;
            self.0.extend_from_slice(chunk);
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
#[cfg(test)]
mod tests;
