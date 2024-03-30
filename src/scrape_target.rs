use std::{future::Future, io, pin::Pin, sync::Arc, time::Duration};

use tokio::{
    sync::Mutex,
    time::{error::Elapsed, Instant},
};

pub type FutureScrapeResult<T> = Pin<Box<dyn Future<Output = ScrapeResult<T>>>>;

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
pub trait ScrapeService {
    type Response;
    fn call(&mut self) -> FutureScrapeResult<Self::Response>;
}

pub type ScrapeResult<T> = Result<T, ScrapeErr>;

pub enum ScrapeOk {
    HttpResponse(reqwest::Response),
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
}

pub struct Timeout<T> {
    inner: T,
    timeout: Duration,
}

impl<T> Timeout<T> {
    pub fn new(inner: T, timeout: Duration) -> Self {
        Self { inner, timeout }
    }
}

impl<T> ScrapeService for Timeout<T>
where
    T: ScrapeService,
    T::Response: 'static,
{
    type Response = T::Response;
    fn call(&mut self) -> FutureScrapeResult<Self::Response> {
        let timeout = self.timeout;
        let call = self.inner.call();
        Box::pin(async move { tokio::time::timeout(timeout, call).await? })
    }
}

/// A scrape target is a pair if scrape services ([ScheduledScrapeTarget],
/// [UnscheduledScrapeTarget]). Calls to the first one resolve at the specified
/// rate _at most_, while calls to the second delay the schedule and resolve as
/// soon as possible.
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
pub fn create_scrape_target<T>(
    inner: T,
    interval: Duration,
) -> (ScheduledScrapeTarget<T>, UnscheduledScrapeTarget<T>) {
    let inner = Arc::new(Mutex::new(SyncedService {
        inner,
        wakeup: Instant::now(),
        interval,
    }));

    (
        ScheduledScrapeTarget {
            inner: inner.clone(),
        },
        UnscheduledScrapeTarget { inner },
    )
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
}

impl<T> ScrapeService for ScheduledScrapeTarget<T>
where
    T: ScrapeService + 'static,
{
    type Response = T::Response;
    fn call(&mut self) -> FutureScrapeResult<Self::Response> {
        let inner = self.inner.clone();
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
        let (mut sched, mut unsched) = create_scrape_target(service, Duration::from_millis(50));
        for i in 0..5 {
            assert_eq!(i, sched.call().await.unwrap());
        }
        assert_eq!(5, unsched.call().await.unwrap());
        for i in 6..10 {
            assert_eq!(i, sched.call().await.unwrap());
        }
        assert!(matches!(
            unsched.call().await,
            ScrapeResult::Err(ScrapeErr::Timeout(_))
        ));
        for i in 11..15 {
            assert_eq!(i, sched.call().await.unwrap());
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
