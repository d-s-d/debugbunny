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
async fn two_http_and_one_command() {
    let server = Server::run();
    let metrics_path = "/metrics";
    let ladygaga_path = "/ladygaga";
    let metrics_reponse = "hello world";
    let ladygaga_response = "whatever";
    let command_out = "hello world from command";

    server.expect(
        Expectation::matching(request::method_path("GET", metrics_path))
            .times(1..)
            .respond_with(status_code(200).body(metrics_reponse.as_bytes().to_vec())),
    );
    server.expect(
        Expectation::matching(request::method_path("GET", ladygaga_path))
            .times(1..)
            .respond_with(status_code(200).body(ladygaga_response.as_bytes().to_vec())),
    );

    let uri = server.url(metrics_path);
    // xxx(dsd): whowever got the idea of returning a URI (!) from a method
    // called url.
    let metrics_url = Url::parse(&uri.to_string()).unwrap();

    let uri = server.url(ladygaga_path);
    // xxx(dsd): whowever got the idea of returning a URI (!) from a method
    // called url.
    let ladygaga_url = Url::parse(&uri.to_string()).unwrap();

    let mut config = Config::new();
    let half_sec = Duration::from_millis(500);
    let quarter_sec = half_sec / 2;
    config.add_target(
        ScrapeTargetBuilder::new()
            .interval(half_sec)
            .timeout(quarter_sec)
            .action(Action::http(metrics_url.clone()))
            .build(),
    );
    config.add_target(
        ScrapeTargetBuilder::new()
            .interval(half_sec)
            .timeout(quarter_sec)
            .action(Action::http(ladygaga_url.clone()))
            .build(),
    );
    config.add_target(
        ScrapeTargetBuilder::new()
            .interval(half_sec)
            .timeout(quarter_sec)
            .action(Action::command_with_args("echo", vec![command_out]))
            .build(),
    );

    let collector = ResultCollector::default();
    let debugbunny =
        DebugBunny::start_scraping(config.clone().scrape_targets, collector.clone()).await;

    tokio::time::sleep(Duration::from_millis(250)).await;
    debugbunny.stop();
    debugbunny.await_shutdown().await;

    assert!(collector.results.lock().await.iter().any(|(c, r)| matches!(
            (c, r),
            (ScrapeTargetConfig {
                action: Action::Http { 
                    url,
                    .. },
                ..
            },
            Ok(ScrapeOk::HttpResponse(resp))) if resp.body() == metrics_reponse.as_bytes() && *url == metrics_url)));
    assert!(collector.results.lock().await.iter().any(|(c, r)| matches!(
            (c, r),
            (ScrapeTargetConfig {
                action: Action::Http { 
                    url,
                    .. },
                ..
            },
            Ok(ScrapeOk::HttpResponse(resp))) if resp.body() == ladygaga_response.as_bytes() && *url == ladygaga_url)));
    assert!(collector.results.lock().await.iter().any(|(c, r)| matches!(
            (c, r),
            (ScrapeTargetConfig {
                action: Action::Command { .. },
                ..
            },
            Ok(ScrapeOk::CommandResponse(out))) if out.stdout.windows(command_out.len()).any(|w| w == command_out.as_bytes()))));
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
