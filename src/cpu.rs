//! Process-wide admission and cooperative cancellation for blocking work.
use crate::{Error, Result};
use std::{
    cell::RefCell,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, MutexGuard, OnceLock, TryLockError,
    },
    time::Instant,
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

#[derive(Clone)]
struct Control {
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
}
thread_local! {
    static CURRENT: RefCell<Option<Control>> = const { RefCell::new(None) };
    static ADMISSION: RefCell<Option<Admission>> = const { RefCell::new(None) };
}
struct Admission {
    slots: Arc<Semaphore>,
    permit: Option<OwnedSemaphorePermit>,
}
struct RestoreAdmission(Option<Admission>);
impl Drop for RestoreAdmission {
    fn drop(&mut self) {
        ADMISSION.with(|slot| {
            slot.replace(self.0.take());
        });
    }
}
fn admitted<T>(
    control: Control,
    slots: Arc<Semaphore>,
    permit: OwnedSemaphorePermit,
    work: impl FnOnce() -> Result<T>,
) -> Result<T> {
    let _restore = RestoreAdmission(ADMISSION.with(|slot| {
        slot.replace(Some(Admission {
            slots,
            permit: Some(permit),
        }))
    }));
    controlled(control, work)
}
/// A contended cache lock parks a blocking worker, not a CPU permit. The
/// worker must regain admission before inspecting or computing any data.
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> Result<MutexGuard<'_, T>> {
    check()?;
    match mutex.try_lock() {
        Ok(guard) => return Ok(guard),
        Err(TryLockError::Poisoned(_)) => {
            return Err(Error::new("TASK_ERROR", "Computation lock poisoned"))
        }
        Err(TryLockError::WouldBlock) => {}
    }
    let mut admission = ADMISSION.with(|slot| slot.borrow_mut().take());
    if let Some(admission) = &mut admission {
        admission.permit.take();
    }
    let waited = (|| loop {
        check()?;
        match mutex.try_lock() {
            Ok(guard) => return Ok(guard),
            Err(TryLockError::Poisoned(_)) => {
                return Err(Error::new("TASK_ERROR", "Computation lock poisoned"))
            }
            Err(TryLockError::WouldBlock) => {
                std::thread::sleep(std::time::Duration::from_millis(5))
            }
        }
    })();
    // Even a poisoned-lock error must restore admission before a caller can
    // handle it and continue. Cancelled work exits without reacquiring a slot.
    check()?;
    if let Some(mut admission) = admission {
        let slots = admission.slots.clone();
        let permit = tokio::runtime::Handle::current().block_on(async move {
            let pending = slots.acquire_owned();
            tokio::pin!(pending);
            loop {
                tokio::select! {
                    result = &mut pending => return result.map_err(|_| Error::new("BUSY", "计算队列已关闭")),
                    _ = tokio::time::sleep(std::time::Duration::from_millis(5)) => check()?,
                }
            }
        })?;
        check()?;
        admission.permit = Some(permit);
        ADMISSION.with(|slot| {
            slot.replace(Some(admission));
        });
    }
    waited
}
fn timeout() -> Error {
    Error {
        status: 504,
        details: serde_json::json!({"source":"cpu"}),
        ..Error::new("TIMEOUT", "计算等待已取消或超过总时限")
    }
}
pub(crate) fn stopped() -> bool {
    CURRENT.with(|slot| {
        slot.borrow()
            .as_ref()
            .is_some_and(|c| c.cancelled.load(Ordering::Relaxed) || Instant::now() >= c.deadline)
    })
}
pub(crate) fn check() -> Result<()> {
    if stopped() {
        Err(timeout())
    } else {
        Ok(())
    }
}
struct CancelOnDrop(Control);
impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.cancelled.store(true, Ordering::Relaxed);
    }
}
struct Reset(Option<Control>);
impl Drop for Reset {
    fn drop(&mut self) {
        CURRENT.with(|slot| {
            slot.replace(self.0.take());
        });
    }
}
fn controlled<T>(control: Control, work: impl FnOnce() -> Result<T>) -> Result<T> {
    let _reset = Reset(CURRENT.with(|slot| slot.replace(Some(control))));
    check()?;
    let result = work();
    // Cancellation must not be swallowed by a finite candidate search.
    check()?;
    result
}

struct Pool {
    slots: Arc<Semaphore>,
    limit: usize,
}
impl Pool {
    fn new(limit: usize) -> Self {
        Self {
            slots: Arc::new(Semaphore::new(limit)),
            limit,
        }
    }
    async fn run<T, F>(&self, deadline: Instant, work: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce() -> Result<T> + Send + 'static,
    {
        let control = Control {
            deadline,
            cancelled: Arc::new(AtomicBool::new(false)),
        };
        let _cancel = CancelOnDrop(control.clone());
        tokio::time::timeout_at(deadline.into(), async {
            let permit = self
                .slots
                .clone()
                .acquire_owned()
                .await
                .map_err(|_| Error::new("BUSY", "计算队列已关闭"))?;
            let slots = self.slots.clone();
            tokio::task::spawn_blocking(move || {
                // Admission remains held until actual exit, except while
                // explicitly parked on a cache lock.
                admitted(control, slots, permit, work)
            })
            .await
            .map_err(|e| Error::new("TASK_ERROR", e.to_string()))?
        })
        .await
        .map_err(|_| timeout())?
    }
    async fn group<T, F>(&self, deadline: Instant, work: F) -> Result<Vec<T>>
    where
        T: Send + 'static,
        F: Fn(usize, usize) -> Result<T> + Send + Sync + 'static,
    {
        let count = self.limit;
        let control = Control {
            deadline,
            cancelled: Arc::new(AtomicBool::new(false)),
        };
        let _cancel = CancelOnDrop(control.clone());
        tokio::time::timeout_at(deadline.into(), async {
            let mut permits = self
                .slots
                .clone()
                .acquire_many_owned(count as u32)
                .await
                .map_err(|_| Error::new("BUSY", "计算队列已关闭"))?;
            let work = Arc::new(work);
            let mut workers = tokio::task::JoinSet::new();
            for index in 0..count {
                let (permit, slots, control, work) = (
                    permits.split(1).unwrap(),
                    self.slots.clone(),
                    control.clone(),
                    work.clone(),
                );
                workers.spawn_blocking(move || {
                    admitted(control, slots, permit, || work(index, count)).map(|v| (index, v))
                });
            }
            drop(permits);
            let mut results = Vec::new();
            while let Some(result) = workers.join_next().await {
                results.push(result.map_err(|e| Error::new("WORK_ERROR", e.to_string()))??);
            }
            results.sort_by_key(|(index, _)| *index);
            Ok(results.into_iter().map(|(_, v)| v).collect())
        })
        .await
        .map_err(|_| timeout())?
    }
}
fn available_cpus() -> usize {
    let available = std::thread::available_parallelism().map_or(1, usize::from);
    #[cfg(windows)]
    {
        // On Windows available_parallelism may describe the machine even when
        // the process is restricted to fewer CPUs by its affinity mask.
        #[link(name = "kernel32")]
        extern "system" {
            fn GetCurrentProcess() -> *mut std::ffi::c_void;
            fn GetProcessAffinityMask(
                process: *mut std::ffi::c_void,
                process_mask: *mut usize,
                system_mask: *mut usize,
            ) -> i32;
        }
        let (mut process_mask, mut system_mask) = (0usize, 0usize);
        // Both outputs are pointer-sized masks owned by this stack frame.
        if unsafe {
            GetProcessAffinityMask(GetCurrentProcess(), &mut process_mask, &mut system_mask)
        } != 0
            && process_mask != 0
        {
            return available.min(process_mask.count_ones() as usize);
        }
    }
    available
}
fn pool() -> &'static Pool {
    static POOL: OnceLock<Pool> = OnceLock::new();
    POOL.get_or_init(|| Pool::new(available_cpus().clamp(1, 6)))
}
pub(crate) fn limit() -> usize {
    pool().limit
}
pub(crate) async fn run<T, F>(deadline: Instant, work: F) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T> + Send + 'static,
{
    pool().run(deadline, work).await
}
pub(crate) async fn group<T, F>(deadline: Instant, work: F) -> Result<Vec<T>>
where
    T: Send + 'static,
    F: Fn(usize, usize) -> Result<T> + Send + Sync + 'static,
{
    pool().group(deadline, work).await
}

pub(crate) fn parse(parser: &mut tree_sitter::Parser, source: &str) -> Result<tree_sitter::Tree> {
    parse_reusing(parser, source, None)
}
pub(crate) fn parse_reusing(
    parser: &mut tree_sitter::Parser,
    source: &str,
    previous: Option<&tree_sitter::Tree>,
) -> Result<tree_sitter::Tree> {
    check()?;
    let mut progress = |_: &tree_sitter::ParseState| {
        if stopped() {
            std::ops::ControlFlow::Break(())
        } else {
            std::ops::ControlFlow::Continue(())
        }
    };
    let tree = parser.parse_with_options(
        &mut |offset, _| &source.as_bytes()[offset..],
        previous,
        Some(tree_sitter::ParseOptions::new().progress_callback(&mut progress)),
    );
    check()?;
    tree.ok_or_else(|| Error::new("PARSER_ERROR", "Script parse failed"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{sync::atomic::AtomicUsize, time::Duration};
    fn deadline() -> Instant {
        Instant::now() + Duration::from_secs(3)
    }

    #[tokio::test]
    async fn cache_waiter_returns_its_only_slot_and_reacquires_before_resuming() {
        let pool = Arc::new(Pool::new(1));
        let cache = Arc::new(Mutex::new(0));
        let owner = cache.lock().unwrap();
        let (started, ready) = tokio::sync::oneshot::channel();
        let (p, c) = (pool.clone(), cache.clone());
        let waiter = tokio::spawn(async move {
            p.run(deadline(), move || {
                let _ = started.send(());
                let mut cached = lock(&c)?;
                ADMISSION.with(|slot| assert!(slot.borrow().as_ref().unwrap().permit.is_some()));
                *cached += 1;
                Ok(())
            })
            .await
        });
        ready.await.unwrap();
        // This work must run while the cache remains locked, even with one CPU.
        tokio::time::timeout(Duration::from_secs(1), pool.run(deadline(), || Ok(42)))
            .await
            .expect("cache waiters must not consume CPU capacity")
            .unwrap();
        assert!(!waiter.is_finished());
        assert_eq!(pool.slots.available_permits(), 1);
        drop(owner);
        waiter.await.unwrap().unwrap();
        assert_eq!(*cache.lock().unwrap(), 1);
        assert_eq!(pool.slots.available_permits(), 1);
    }

    #[tokio::test]
    async fn cancellation_during_readmission_releases_the_cache_lock_without_running_work() {
        let pool = Arc::new(Pool::new(1));
        let cache = Arc::new(Mutex::new(()));
        let owner = cache.lock().unwrap();
        let resumed = Arc::new(AtomicBool::new(false));
        let (started, ready) = tokio::sync::oneshot::channel();
        let (p, c, done) = (pool.clone(), cache.clone(), resumed.clone());
        let waiter = tokio::spawn(async move {
            p.run(deadline(), move || {
                let _ = started.send(());
                let _guard = lock(&c)?;
                done.store(true, Ordering::SeqCst);
                Ok(())
            })
            .await
        });
        ready.await.unwrap();
        let permit =
            tokio::time::timeout(Duration::from_secs(1), pool.slots.clone().acquire_owned())
                .await
                .unwrap()
                .unwrap();
        drop(owner);
        // Wait until the worker has acquired the cache but is still queued for
        // CPU admission. It must be cancellable in this state as well.
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if cache.try_lock().is_err() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        })
        .await
        .unwrap();
        waiter.abort();
        let _ = waiter.await;
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if cache.try_lock().is_ok() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        })
        .await
        .expect("cancelled readmission must drop the cache lock");
        assert!(!resumed.load(Ordering::SeqCst));
        drop(permit);
        pool.run(deadline(), || Ok(())).await.unwrap();
        assert_eq!(pool.slots.available_permits(), 1);
    }

    #[tokio::test]
    async fn dropping_request_stops_running_work_and_releases_its_slot() {
        let pool = Arc::new(Pool::new(1));
        let (started, ready) = tokio::sync::oneshot::channel();
        let exited = Arc::new(AtomicBool::new(false));
        let done = exited.clone();
        let p = pool.clone();
        let request = tokio::spawn(async move {
            p.run(deadline(), move || {
                let _ = started.send(());
                loop {
                    if let Err(error) = check() {
                        done.store(true, Ordering::SeqCst);
                        return Err::<(), _>(error);
                    }
                    std::thread::yield_now();
                }
            })
            .await
        });
        ready.await.unwrap();
        request.abort();
        assert!(request.await.unwrap_err().is_cancelled());
        // A second job cannot start until the cancelled closure releases it.
        pool.run(deadline(), move || {
            assert!(exited.load(Ordering::SeqCst));
            Ok(())
        })
        .await
        .unwrap();
        assert_eq!(pool.slots.available_permits(), 1);
    }

    #[tokio::test]
    async fn expired_queued_job_never_starts() {
        let pool = Pool::new(1);
        let permit = pool.slots.clone().acquire_owned().await.unwrap();
        let invoked = Arc::new(AtomicBool::new(false));
        let observed = invoked.clone();
        let error = pool
            .run(Instant::now() + Duration::from_millis(10), move || {
                observed.store(true, Ordering::SeqCst);
                Ok(())
            })
            .await
            .unwrap_err();
        assert_eq!(error.code, "TIMEOUT");
        drop(permit);
        pool.run(deadline(), || Ok(())).await.unwrap();
        assert!(!invoked.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn groups_from_distinct_requests_share_the_same_capacity() {
        let pool = Arc::new(Pool::new(2));
        let active = Arc::new(AtomicUsize::new(0));
        let maximum = Arc::new(AtomicUsize::new(0));
        let mut requests = tokio::task::JoinSet::new();
        for _ in 0..4 {
            let (pool, active, maximum) = (pool.clone(), active.clone(), maximum.clone());
            requests.spawn(async move {
                pool.group(deadline(), move |_, count| {
                    assert_eq!(count, 2);
                    let current = active.fetch_add(1, Ordering::SeqCst) + 1;
                    maximum.fetch_max(current, Ordering::SeqCst);
                    std::thread::sleep(Duration::from_millis(10));
                    active.fetch_sub(1, Ordering::SeqCst);
                    Ok(())
                })
                .await
                .unwrap()
            });
        }
        while let Some(result) = requests.join_next().await {
            assert_eq!(result.unwrap().len(), 2);
        }
        assert!(maximum.load(Ordering::SeqCst) <= 2);
        assert_eq!(active.load(Ordering::SeqCst), 0);
        assert_eq!(pool.slots.available_permits(), 2);
    }

    #[tokio::test]
    async fn cancelling_a_large_syntax_parse_releases_the_cpu_slot_promptly() {
        let pool = Arc::new(Pool::new(1));
        let source = "var a=1;".repeat(500_000);
        let (started, ready) = tokio::sync::oneshot::channel();
        let p = pool.clone();
        let request = tokio::spawn(async move {
            p.run(deadline(), move || {
                let mut parser = tree_sitter::Parser::new();
                parser
                    .set_language(&tree_sitter_javascript::LANGUAGE.into())
                    .unwrap();
                let _ = started.send(());
                parse(&mut parser, &source).map(|_| ())
            })
            .await
        });
        ready.await.unwrap();
        tokio::time::sleep(Duration::from_millis(10)).await;
        request.abort();
        let _ = request.await;
        tokio::time::timeout(Duration::from_millis(500), pool.run(deadline(), || Ok(())))
            .await
            .expect("tree-sitter progress callback must stop an abandoned parse")
            .unwrap();
    }

    #[tokio::test]
    async fn cancelled_proof_group_releases_all_slots_and_respects_cpu_limit() {
        for count in [1, 2] {
            let pool = Arc::new(Pool::new(count));
            let started = Arc::new(AtomicUsize::new(0));
            let (p, observed) = (pool.clone(), started.clone());
            let request = tokio::spawn(async move {
                p.group(deadline(), move |index, workers| {
                    assert_eq!(workers, count);
                    observed.fetch_add(1, Ordering::SeqCst);
                    crate::wire::work(
                        "bbbbbbbbbbbbbbbb|1792490000123|0|",
                        "suffix",
                        1000,
                        index as u64,
                        workers as u64,
                    )
                })
                .await
            });
            tokio::time::timeout(Duration::from_secs(2), async {
                while started.load(Ordering::SeqCst) < count {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
            request.abort();
            let _ = request.await;
            tokio::time::timeout(Duration::from_millis(500), pool.run(deadline(), || Ok(())))
                .await
                .expect("proof loop should observe cancellation before its one-second budget")
                .unwrap();
            let permits = tokio::time::timeout(
                Duration::from_millis(500),
                pool.slots.clone().acquire_many_owned(count as u32),
            )
            .await
            .unwrap()
            .unwrap();
            drop(permits);
            assert_eq!(pool.slots.available_permits(), count);
        }
    }

    #[test]
    fn expired_context_prevents_work_and_does_not_leak_to_thread_reuse() {
        let control = Control {
            cancelled: Arc::new(AtomicBool::new(false)),
            deadline: Instant::now(),
        };
        let result = controlled(control, || {
            crate::compiler::compile("function f(){return 1}", false)
        });
        assert_eq!(result.err().unwrap().code, "TIMEOUT");
        assert!(!stopped());
        assert!(check().is_ok());
    }
    #[tokio::test]
    async fn configuration_parsers_cancel_without_blocking_async_requests() {
        for parser in 0..3 {
            let pool = Arc::new(Pool::new(1));
            let source = "var a=1;".repeat(500_000) + ";window._cf_chl_opt={value:42}";
            let (started, ready) = tokio::sync::oneshot::channel();
            let p = pool.clone();
            let request = tokio::spawn(async move {
                p.run(deadline(), move || {
                    let _ = started.send(());
                    match parser {
                        0 => crate::static_data::assignment(&source, "_cf_chl_opt"),
                        1 => crate::static_data::object_data(&source, "_cf_chl_opt"),
                        _ => crate::jsd::inspect(&source).map(Some),
                    }
                })
                .await
            });
            ready.await.unwrap();
            // This timer runs on a single async worker while parsing continues.
            tokio::time::sleep(Duration::from_millis(10)).await;
            assert!(!request.is_finished());
            request.abort();
            let _ = request.await;
            tokio::time::timeout(Duration::from_secs(1), pool.run(deadline(), || Ok(())))
                .await
                .expect("configuration parser must release the CPU slot after cancellation")
                .unwrap();
        }
    }
    #[tokio::test]
    async fn protocol_decode_and_literal_scan_leave_async_runtime_responsive_and_cancel() {
        use base64::Engine as _;
        let format: crate::data::Format = serde_json::from_value(serde_json::json!({
            "byte_offset":0,"initial_key":0,"state_step":1,"state_multiplier":17,
            "state_increment":3,"hash_salt":0,"char_xor":0,"state_quadratic":0,"char_mode":"before-subtract"
        })).unwrap();
        for decode in [true, false] {
            let pool = Arc::new(Pool::new(1));
            let encoded = base64::engine::general_purpose::STANDARD.encode(vec![
                0;
                if decode {
                    8 * 1024 * 1024
                } else {
                    512 * 1024
                }
            ]);
            let plan = crate::data::Plan {
                format: format.clone(),
                meta: serde_json::json!({"key_material":{"literal_tag":255,"literal_kind_slot":1}}),
                alphabet: String::new(),
                modulus: String::new(),
                flow: String::new(),
            };
            let (started, ready) = tokio::sync::oneshot::channel();
            let p = pool.clone();
            let request = tokio::spawn(async move {
                p.run(deadline(), move || {
                    let _ = started.send(());
                    loop {
                        if decode {
                            crate::wire::flow(&encoded, "0123456789abcdef")?;
                        } else {
                            crate::data::Response::read(&encoded, &plan, true)?;
                        }
                    }
                    #[allow(unreachable_code)]
                    Ok(())
                })
                .await
            });
            ready.await.unwrap();
            tokio::time::sleep(Duration::from_millis(10)).await;
            assert!(!request.is_finished());
            request.abort();
            let _ = request.await;
            tokio::time::timeout(Duration::from_secs(1), pool.run(deadline(), || Ok(())))
                .await
                .expect("response decoding and literal scans must observe cancellation")
                .unwrap();
        }
    }
}
