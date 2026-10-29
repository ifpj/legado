use super::*;
use std::sync::atomic::{AtomicBool, Ordering};

#[tokio::test]
async fn initial_request_and_validation_progress_concurrently_before_returning_response() {
    let (request_started, request_ready) = tokio::sync::oneshot::channel();
    let (validation_started, validation_ready) = tokio::sync::oneshot::channel();
    let request = async {
        request_started.send(()).unwrap();
        validation_ready.await.unwrap();
        Ok(17u8)
    };
    let validation = async {
        validation_started.send(()).unwrap();
        request_ready.await.unwrap();
        Ok(())
    };
    assert_eq!(
        tokio::time::timeout(
            Duration::from_secs(1),
            validated_response(request, validation)
        )
        .await
        .unwrap()
        .unwrap(),
        17
    );
}

struct Dropped(Arc<AtomicBool>);
impl Drop for Dropped {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

#[tokio::test]
async fn rejected_current_declaration_cancels_inflight_request_and_never_releases_response() {
    let dropped = Arc::new(AtomicBool::new(false));
    let state = dropped.clone();
    let request = async move {
        let _guard = Dropped(state);
        std::future::pending::<Result<u8>>().await
    };
    let validation = async {
        tokio::task::yield_now().await;
        Err(Error::new("INVALID_DATA", "changed current CV"))
    };
    let result = validated_response(request, validation).await;
    assert_eq!(result.unwrap_err().message, "changed current CV");
    assert!(dropped.load(Ordering::SeqCst));
}

#[test]
fn only_component_task_hosts_are_learned_most_recent_first() {
    let task = |url: &str| crate::http_tasks::Task {
        id: String::new(),
        kind: String::new(),
        url: url.into(),
        timestamp: String::new(),
        digest: String::new(),
    };
    TASK_HOSTS.lock().unwrap().clear();
    learn_task_hosts(&[
        task("https://challenges.cloudflare.com/cdn-cgi/challenge-platform/h/b/pat/1"),
        task("https://hagen.challenges.cloudflare.com/cdn-cgi/challenge-platform/h/b/i/1"),
        task("https://example.com/cdn-cgi/challenge-platform/h/b/i/1"),
        task("https://a.b.challenges.cloudflare.com/i"),
        task("not a url"),
    ]);
    assert_eq!(learned_task_hosts(), ["hagen.challenges.cloudflare.com"]);
    learn_task_hosts(&[
        task("https://first.challenges.cloudflare.com/i"),
        task("https://second.challenges.cloudflare.com/i"),
    ]);
    assert_eq!(
        learned_task_hosts(),
        [
            "second.challenges.cloudflare.com",
            "first.challenges.cloudflare.com"
        ]
    );
    TASK_HOSTS.lock().unwrap().clear();
}
