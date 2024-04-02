use std::{sync::{Arc, Mutex}, time::Duration};

use tokio::{task::JoinHandle};

use crate::{
    command::new_from_config,
    config::ScrapeTargetConfig,
    http::HttpScrapeTarget,
    result_processor::ScrapeResultProcessor,
    scrape_target::{create_scrape_target, BoxedScrapeService, ScrapeService, Timeout},
};

pub struct TargetCollection {
    configs: Vec<ScrapeTargetConfig>,
    scheduled_tasks: Vec<JoinHandle<()>>,
    unscheduled_targets: Vec<Arc<Mutex<BoxedScrapeService>>>,
}

impl TargetCollection {
    pub async fn start_scraping<P: ScrapeResultProcessor + 'static>(
        configs: Vec<ScrapeTargetConfig>,
        p: P,
    ) -> Self {
        use crate::config::Action::*;
        let client = reqwest::Client::new();
        let (scheduled_tasks, unscheduled_targets): (Vec<_>, Vec<_>) = configs
            .iter()
            .map(|c| {
                // action layer
                let s = match &c.action {
                    Http { url, .. } => {
                        Box::new(HttpScrapeTarget::new(client.clone(), url.clone()))
                            as BoxedScrapeService
                    }
                    Command { command, args } => {
                        Box::new(new_from_config(command.clone(), args.clone()))
                            as BoxedScrapeService
                    }
                };
                // timeout
                let t = Timeout::new(s, c.timeout.unwrap_or(Duration::from_secs(2)));
                let (mut s, u) = create_scrape_target(t, c.interval);

                // scheduled driver
                let scheduled = tokio::task::spawn({
                    let p = p.clone();
                    let c = c.clone();
                    async move {
                        loop {
                            if let Err(e) = p.process(&c, s.call().await).await {
                                eprintln!("Error: {e:?}");
                            }
                        }
                    }
                });

                (
                    scheduled,
                    Arc::new(Mutex::new(Box::new(u) as BoxedScrapeService)),
                )
            })
            .unzip();

        Self {
            configs,
            scheduled_tasks,
            unscheduled_targets,
        }
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
        for jh in self.scheduled_tasks {
            if let Err(e) = jh.await {
                eprintln!("Error: {e:?}");
            }
        }
    }
}
