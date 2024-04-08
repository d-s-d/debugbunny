use std::time::Duration;

use debugbunny::{
    config::{Action, Config, ScrapeTargetBuilder},
    debugbunny::DebugBunny,
    result_processor::LogOutputWriter,
};
use tokio::{io::stderr, signal};
use url::Url;

#[tokio::main]
async fn main() {
    let mut config = Config::new();
    let half_min = Duration::from_secs(30);
    let quarter_min = half_min / 2;
    let quarter_sec = Duration::from_millis(250);
    let url = Url::parse("http://localhost:8080/system_status").unwrap();
    config.add_target(
        ScrapeTargetBuilder::new()
            .interval(quarter_min)
            .timeout(quarter_sec)
            .action(Action::http(url))
            .build(),
    );
    // Use `ss` to list all open listening TCP-sockets ...
    config.add_target(
        ScrapeTargetBuilder::new()
            .interval(half_min)
            .timeout(quarter_sec)
            .action(Action::command_with_args("/usr/bin/ss", vec!["-s", "-l"]))
            .build(),
    );

    let stderr = stderr();
    let p = LogOutputWriter::new(stderr);
    let debugbunny = DebugBunny::start_scraping(config.scrape_targets, p).await;

    // Wait for the SIGTERM signal
    match signal::unix::signal(signal::unix::SignalKind::terminate()) {
        Ok(mut sigterm) => {
            sigterm.recv().await;
            println!("SIGTERM received, performing graceful shutdown ...");
            debugbunny.stop();
            debugbunny.await_shutdown().await;
        }
        Err(e) => eprintln!("Unable to listen for SIGTERM signals: {:?}", e),
    }
}
