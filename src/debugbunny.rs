use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

use tokio::task::JoinHandle;

use crate::{
    command::new_from_config,
    config::ScrapeTargetConfig,
    http::HttpScrapeTarget,
    result_processor::ScrapeResultProcessor,
    scrape_target::{create_scrape_target, BoxedScrapeService, ScrapeOk, ScrapeService, Timeout},
};

pub struct DebugBunny {
    should_run: Arc<AtomicBool>,
    configs: Vec<ScrapeTargetConfig>,
    scheduled_tasks: Vec<JoinHandle<()>>,
    unscheduled_targets: Vec<Arc<Mutex<BoxedScrapeService>>>,
}

impl DebugBunny {
    pub async fn start_scraping<P: ScrapeResultProcessor + 'static>(
        configs: Vec<ScrapeTargetConfig>,
        p: P,
    ) -> Self {
        use crate::config::Action::*;
        let should_run = Arc::new(AtomicBool::new(true));
        let client = reqwest::Client::new();
        let (scheduled_tasks, unscheduled_targets): (Vec<_>, Vec<_>) = configs
            .iter()
            .map(|c| match &c.action {
                Http { url, .. } => {
                    let s = HttpScrapeTarget::new(client.clone(), url.clone());
                    Self::launch_scheduled_task(s, p.clone(), c, should_run.clone())
                }
                Command { command, args } => {
                    let s = new_from_config(command.clone(), args.clone());
                    Self::launch_scheduled_task(s, p.clone(), c, should_run.clone())
                }
            })
            .unzip();

        let unscheduled_targets = unscheduled_targets
            .into_iter()
            .map(|s| Arc::new(Mutex::new(s)))
            .collect();
        Self {
            configs,
            scheduled_tasks,
            unscheduled_targets,
            should_run,
        }
    }

    fn launch_scheduled_task<S, P>(
        s: S,
        p: P,
        c: &ScrapeTargetConfig,
        should_run: Arc<AtomicBool>,
    ) -> (JoinHandle<()>, BoxedScrapeService)
    where
        S: ScrapeService<Response = ScrapeOk> + 'static,
        P: ScrapeResultProcessor + 'static,
    {
        let t = Timeout::new(s, c.timeout.unwrap_or(Duration::from_secs(2)));
        let (mut s, u) = create_scrape_target(t, c.interval);

        // scheduled driver
        let scheduled = tokio::task::spawn({
            let p = p.clone();
            let c = c.clone();
            let should_run = should_run.clone();
            async move {
                while should_run.load(Ordering::Relaxed) {
                    if let Err(e) = p.process(&c, s.call().await).await {
                        eprintln!("Error: {e:?}");
                    }
                }
            }
        });
        (scheduled, Box::new(u))
    }

    pub async fn unscheduled_call<P: ScrapeResultProcessor + 'static>(&self, p: P) {
        let mut jhs = vec![];
        for (c, u) in self.configs.iter().zip(self.unscheduled_targets.iter()) {
            let jh = tokio::task::spawn({
                let p = p.clone();
                let c = c.clone();
                let u = u.clone();
                async move {
                    let f = u.lock().unwrap().call();
                    if let Err(e) = p.process(&c, f.await).await {
                        eprintln!("Error: {e:?}");
                    }
                }
            });
            jhs.push(jh);
        }
        for jh in jhs {
            if let Err(e) = jh.await {
                eprintln!("Error: {e:?}");
            }
        }
    }

    pub async fn await_shutdown(self) {
        self.should_run.fetch_and(false, Ordering::Relaxed);
        for jh in self.scheduled_tasks {
            if let Err(e) = jh.await {
                eprintln!("Error: {e:?}");
            }
        }
    }
}
