use std::{sync::Arc, time::Duration};

use debugbunny::{
    config::{Action, Config, ScrapeTargetBuilder, ScrapeTargetConfig},
    debugbunny::DebugBunny,
    result_processor::ScrapeResultProcessor,
    scrape_target::{ScrapeOk, ScrapeResult},
};
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

    let uri = server.url("/metrics");
    let url = Url::parse(&uri.to_string()).unwrap();

    let mut config = Config::new();
    let half_sec = Duration::from_millis(500);
    let quarter_sec = half_sec / 2;
    config.add_target(
        ScrapeTargetBuilder::new()
            .interval(half_sec)
            .timeout(quarter_sec)
            .action(Action::http(url))
            .build(),
    );
    config.add_target(
        ScrapeTargetBuilder::new()
            .interval(half_sec)
            .timeout(quarter_sec)
            .action(Action::command_with_args(
                "echo",
                vec!["hello world from command"],
            ))
            .build(),
    );

    let collector = ResultCollector::default();
    let debugbunny =
        DebugBunny::start_scraping(config.clone().scrape_targets, collector.clone()).await;

    tokio::time::sleep(Duration::from_millis(250)).await;
    debugbunny.await_shutdown().await;

    assert!(collector.results.lock().await.iter().any(|(c, r)| matches!(
            (c, r),
            (ScrapeTargetConfig {
                action: Action::Http { .. },
                ..
            },
            Ok(ScrapeOk::HttpResponse(resp))) if resp.body() == "hello world".as_bytes())));
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
