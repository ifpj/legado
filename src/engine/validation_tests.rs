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
