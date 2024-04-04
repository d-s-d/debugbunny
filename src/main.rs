use std::time::Duration;

use debugbunny::{
    config::{Action, Config, ScrapeTargetBuilder},
    debugbunny::DebugBunny,
    result_processor::LogOutputWriter,
};
use tokio::io::stderr;
use url::Url;

#[tokio::main]
async fn main() {
    let mut config = Config::new();
    let half_sec = Duration::from_millis(500);
    let quarter_sec = half_sec / 2;

    let url = Url::parse("http://127.0.0.1:8000/README.md").unwrap();
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

    let stderr = stderr();
    let p = LogOutputWriter::new(stderr);
    let _debugbunny = DebugBunny::start_scraping(config.scrape_targets, p).await;

    loop {
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}
