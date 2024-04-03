use std::{sync::Arc, time::Duration};

use debugbunny::{
    config::{Action, Config, ScrapeTargetConfig},
    debugbunny::DebugBunny,
    result_processor::ScrapeResultProcessor,
    scrape_target::{ScrapeOk, ScrapeResult},
};
use http::Method;
use httptest::{matchers::*, responders::*, Expectation, Server};
use tokio::sync::Mutex;
use url::Url;

#[tokio::test]
async fn asdf() {
    let server = Server::run();
    server.expect(
        Expectation::matching(request::method_path("GET", "/metrics"))
            .times(1..)
            .respond_with(status_code(200).body("hello world".as_bytes().to_vec())),
    );

    let url = server.url("/metrics");
    let config = Config {
        scrape_targets: vec![
            ScrapeTargetConfig {
                interval: Duration::from_millis(500),
                timeout: Some(Duration::from_millis(200)),
                action: Action::Http {
                    method: Some(Method::GET),
                    url: Url::parse(&url.to_string()).unwrap(),
                },
            },
            ScrapeTargetConfig {
                interval: Duration::from_millis(500),
                timeout: Some(Duration::from_millis(200)),
                action: Action::Command {
                    command: "echo".to_string(),
                    args: vec!["hello world from command".to_string()],
                },
            },
        ],
    };

    let collector = ResultCollector::default();
    let debugbunny =
        DebugBunny::start_scraping(config.clone().scrape_targets, collector.clone()).await;

    tokio::time::sleep(Duration::from_millis(250)).await;
    debugbunny.await_shutdown().await;

    assert!(collector
        .results
        .lock()
        .await
        .iter()
        .filter(|(c, r)| matches!(
            (c, r),
            (ScrapeTargetConfig {
                action: Action::Http { .. },
                ..
            },
            Ok(ScrapeOk::HttpResponse(resp))) if resp.body() == "hello world".as_bytes()))
        .next()
        .is_some());
}

type SharedResults = Arc<Mutex<Vec<(ScrapeTargetConfig, ScrapeResult<ScrapeOk>)>>>;

#[derive(Default, Clone)]
struct ResultCollector {
    results: SharedResults,
}

impl ScrapeResultProcessor for ResultCollector {
    async fn process(
        &self,
        config: &ScrapeTargetConfig,
        result: ScrapeResult<ScrapeOk>,
    ) -> std::io::Result<()> {
        let mut guard = self.results.lock().await;
        guard.push((config.clone(), result));
        Ok(())
    }
}
