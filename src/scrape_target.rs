use std::{future::Future, io, pin::Pin, sync::Arc, time::Duration};

use tokio::{
    sync::{
        watch::{Receiver, Sender},
        Mutex,
    },
    time::{error::Elapsed, Instant},
};

pub type FutureScrapeResult<T> = Pin<Box<dyn Future<Output = ScrapeResult<T>> + Send>>;
pub type BoxedScrapeService = Box<dyn ScrapeService<Response = ScrapeOk>>;

/// A [ScrapeService] is a call that eventually produces a scrape result
/// asynchronously. A scrape target is a special case of a scrape service.
///
/// # Implementation notes
///
/// We could have mapped this into tower::Service and we might do that at some
/// point in the future. However, the latter trait is so generic that the
/// boilerplate does not justify the overhead at this moment in time.
//
// xxx(dsd): Further, we might have used async_trait here. In the spirit of
// minimalism, we took the route of returning a future. The result is that we
// expose some low-level details of rust's async semantics.
//
// todo(dsd): test whether we can use `impl Future` the return position here.
pub trait ScrapeService: Send {
    type Response: Send + 'static;
    fn call(&mut self) -> FutureScrapeResult<Self::Response>;
}

// This is the equivalent of
// https://github.com/tower-rs/tower/blob/39adf5c509a1b2141f679654d8317524ca96b58b/tower-service/src/lib.rs#L375
impl<T: ScrapeService + ?Sized> ScrapeService for Box<T> {
    type Response = T::Response;

    fn call(&mut self) -> FutureScrapeResult<Self::Response> {
        (**self).call()
    }
}

pub type ScrapeResult<T> = Result<T, ScrapeErr>;

pub enum ScrapeOk {
    HttpResponse(http::Response<Vec<u8>>),
    CommandResponse(std::process::Output),
}

#[derive(Debug, thiserror::Error)]
pub enum ScrapeErr {
    #[error("Http error")]
    HttpErr(#[from] reqwest::Error),
    // xxx(dsd): this is not entirely clean, as an io-error might occur in other places too.
    #[error("Command execution error")]
    IoErr(#[from] io::Error),
    #[error("Scrape timed out")]
    Timeout(#[from] Elapsed),
    #[error("Cancelled")]
    Cancelled,
}

pub struct Timeout<T> {
    inner: T,
    timeout: Duration,
    cancel: Option<Receiver<()>>,
}

impl<T> Timeout<T> {
    pub fn new(inner: T, timeout: Duration) -> Self {
        Self {
            inner,
            timeout,
            cancel: None,
        }
    }

    pub fn new_with_cancel(inner: T, timeout: Duration, cancel: Receiver<()>) -> Self {
        Self {
            inner,
            timeout,
            cancel: Some(cancel),
        }
    }
}

impl<T> ScrapeService for Timeout<T>
where
    T: ScrapeService,
{
    type Response = T::Response;
    fn call(&mut self) -> FutureScrapeResult<Self::Response> {
        let timeout = self.timeout;
        let call = self.inner.call();
        if let Some(cancel) = &self.cancel {
            let mut cancel = cancel.clone();
            return Box::pin(async move {
                tokio::select! {
                    r = tokio::time::timeout(timeout, call) => r?,
                    _ = cancel.changed() => Err(ScrapeErr::Cancelled)
                }
            });
        }
        Box::pin(async move { tokio::time::timeout(timeout, call).await? })
    }
}

/// A scrape target is essentially a pair if scrape services
/// ([ScheduledScrapeTarget], [UnscheduledScrapeTarget]). Calls to the first one
/// resolve at the specified rate _at most_, while calls to the second delay the
/// schedule and resolve as soon as possible.
///
/// The idea is that the first one can be used in a busy loop to call scrapes at
/// the specified rate.
///
/// # Implementation notes
///
/// One could imagine exposing the functionality using a single object with two
/// methods instead, e.g. `call()` and `call_unscheduled()`. However, this would
/// push the responsibility of scheduling calls to the caller if any of the
/// method calls take an exlusive reference (`&mut`). Using internal mutability
/// (`&self`), on the other hand, incurs the same implementation overhead as the
/// current approach.
pub struct ScrapeTarget<T> {
    pub scheduled: ScheduledScrapeTarget<T>,
    pub unscheduled: UnscheduledScrapeTarget<T>,
    pub cancel_signal: Option<Sender<()>>,
}

impl<T> ScrapeTarget<T> {
    pub fn new(inner: T, interval: Duration) -> Self {
        Self::new_with_cancel_opt(inner, interval, None)
    }

    pub fn new_with_cancel(inner: T, interval: Duration, cancel: Receiver<()>) -> Self {
        Self::new_with_cancel_opt(inner, interval, Some(cancel))
    }

    fn new_with_cancel_opt(inner: T, interval: Duration, cancel: Option<Receiver<()>>) -> Self {
        let inner = Arc::new(Mutex::new(SyncedService {
            inner,
            wakeup: Instant::now(),
            interval,
        }));

        Self {
            scheduled: ScheduledScrapeTarget {
                inner: inner.clone(),
                cancel,
            },
            unscheduled: UnscheduledScrapeTarget { inner },
            cancel_signal: None,
        }
    }
}

struct SyncedService<T> {
    inner: T,
    wakeup: Instant,
    interval: Duration,
}

impl<T> SyncedService<T> {
    /// Sets the wakeup time to the first point in the future that is a multiple
    /// of the current interval using the current schedule.
    fn set_next_wake_up_time(&mut self) {
        let now = Instant::now();
        if now < self.wakeup {
            return;
        }

        let delta = now - self.wakeup;
        let ival_nanos = self.interval.as_nanos();
        let f = ((delta.as_nanos() + ival_nanos) / (ival_nanos)) as u32;
        assert!(f >= 1);
        self.wakeup += self.interval * f;
    }

    /// Resets the schedule to the current point in time. As a result, the next
    /// scheduled scrape is happening one interval from now.
    fn reset_interval(&mut self) {
        self.wakeup = Instant::now();
        self.set_next_wake_up_time();
    }

    fn is_due(&self) -> bool {
        Instant::now() >= self.wakeup
    }
}

/// [ScheduledScrapeTarget] implements the [ScrapeService] trait for any wrapped
/// type `T: ScrapeService`. Repeated calls to `call()` will return at most once
/// per interval.
///
/// A call to the ScrapeService implemented by the 'companion' object
/// [UnscheduledScrapeTarget] belonging to this instance will delay the schedule
/// by _at least_ on interval.
pub struct ScheduledScrapeTarget<T> {
    inner: Arc<Mutex<SyncedService<T>>>,
    cancel: Option<Receiver<()>>,
}

impl<T> ScrapeService for ScheduledScrapeTarget<T>
where
    T: ScrapeService + 'static,
{
    type Response = T::Response;
    fn call(&mut self) -> FutureScrapeResult<Self::Response> {
        let inner = self.inner.clone();
        let mut cancel = self.cancel.clone();
        Box::pin(async move {
            loop {
                let wakeup = {
                    // critical section
                    let mut lockguard = inner.lock().await;
                    if lockguard.is_due() {
                        let res = lockguard.inner.call().await;
                        lockguard.set_next_wake_up_time();
                        break res;
                    }
                    lockguard.wakeup
                };
                if let Some(ref mut cancel) = cancel {
                    tokio::select! {
                        _ = tokio::time::sleep_until(wakeup) => continue,
                        _ = cancel.changed() => break Err(ScrapeErr::Cancelled)
                    }
                }
                tokio::time::sleep_until(wakeup).await;
            }
        })
    }
}

pub struct UnscheduledScrapeTarget<T> {
    inner: Arc<Mutex<SyncedService<T>>>,
}

impl<T> ScrapeService for UnscheduledScrapeTarget<T>
where
    T: ScrapeService + 'static,
{
    type Response = T::Response;
    fn call(&mut self) -> FutureScrapeResult<Self::Response> {
        let inner = self.inner.clone();
        Box::pin(async move {
            let mut lockguard = inner.lock().await;
            let res = lockguard.inner.call().await;
            lockguard.reset_interval();
            res
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn synchronized_timeout_service() {
        let timeout = Duration::from_millis(40);
        let service = Counter(0);
        let service = Timeout::new(service, timeout);
        let mut st = ScrapeTarget::new(service, Duration::from_millis(50));
        for i in 0..5 {
            assert_eq!(i, st.scheduled.call().await.unwrap());
        }
        assert_eq!(5, st.unscheduled.call().await.unwrap());
        for i in 6..10 {
            assert_eq!(i, st.scheduled.call().await.unwrap());
        }
        assert!(matches!(
            st.unscheduled.call().await,
            ScrapeResult::Err(ScrapeErr::Timeout(_))
        ));
        for i in 11..15 {
            assert_eq!(i, st.scheduled.call().await.unwrap());
        }
    }

    struct Counter(usize);

    impl ScrapeService for Counter {
        type Response = usize;

        fn call(&mut self) -> FutureScrapeResult<Self::Response> {
            let c = self.0;
            self.0 += 1;
            Box::pin(async move {
                if c == 10 {
                    tokio::time::sleep(Duration::from_millis(30)).await;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
                Ok(c)
            })
        }
    }
}
