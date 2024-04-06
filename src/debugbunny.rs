use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use tokio::{
    sync::watch::{self, Receiver, Sender},
    task::JoinHandle,
};

use crate::{
    command::new_from_config,
    config::ScrapeTargetConfig,
    http::HttpScrapeTarget,
    result_processor::ScrapeResultProcessor,
    scrape_target::{BoxedScrapeService, ScrapeOk, ScrapeService, ScrapeTarget, Timeout},
};

pub struct DebugBunny {
    configs: Vec<ScrapeTargetConfig>,
    scheduled_tasks: Vec<JoinHandle<()>>,
    unscheduled_targets: Vec<Arc<Mutex<BoxedScrapeService>>>,
    cancel_signal: Sender<()>,
}

impl DebugBunny {
    pub async fn start_scraping<P: ScrapeResultProcessor + 'static>(
        configs: Vec<ScrapeTargetConfig>,
        p: P,
    ) -> Self {
        use crate::config::Action::*;
        let (cancel_signal, cancel) = watch::channel(());
        let client = reqwest::Client::new();
        let (scheduled_tasks, unscheduled_targets): (Vec<_>, Vec<_>) = configs
            .iter()
            .map(|c| match &c.action {
                Http { url, .. } => {
                    let s = HttpScrapeTarget::new(client.clone(), url.clone());
                    Self::launch_scheduled_task(s, p.clone(), c, cancel.clone())
                }
                Command { command, args } => {
                    let s = new_from_config(command.clone(), args.clone());
                    Self::launch_scheduled_task(s, p.clone(), c, cancel.clone())
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
            cancel_signal,
        }
    }

    fn launch_scheduled_task<S, P>(
        s: S,
        p: P,
        c: &ScrapeTargetConfig,
        cancel: Receiver<()>,
    ) -> (JoinHandle<()>, BoxedScrapeService)
    where
        S: ScrapeService<Response = ScrapeOk> + 'static,
        P: ScrapeResultProcessor + 'static,
    {
        let t = Timeout::new_with_cancel(s, c.timeout.unwrap_or(Duration::from_secs(2)), cancel.clone());
        let st = ScrapeTarget::new_with_cancel(t, c.interval, cancel.clone());
        let mut s = st.scheduled;
        let u = st.unscheduled;

        // scheduled driver
        let scheduled = tokio::task::spawn({
            let p = p.clone();
            let c = c.clone();
            let cancel = cancel.clone();
            async move {
                // xxx(dsd): here we just treat receive errors on the signal as
                // a change
                while !cancel.has_changed().unwrap_or(true) {
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

    pub fn stop(&self) {
        let _ = self.cancel_signal.send(());
    }

    pub async fn await_shutdown(self) {
        for jh in self.scheduled_tasks {
            if let Err(e) = jh.await {
                eprintln!("Error: {e:?}");
            }
        }
    }
}
