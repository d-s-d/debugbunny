# 🐰 debugbunny 🛠️

A small toolbox to scrape HTTP-endpoints and commands on a regular basis and
publish the results as log messages.

## Overview

Let's say you operate a service and you want to assess part of the system state
after the fact — debugbunny is here to help you. It scrapes previously specified
_scrape targets_ on a regular interval and emits the results as compressed log
messages (if so configured). As of now, a scrape target is either an
HTTP-endpoint or a shell command.

Some situations where debugbunny might come in handy:

* The target system is a production system that is not attached to your
prometheus monitoring stack and/or you lost connection and you need to debug a
failure — debugbunny allows you to extract collected metric-data after the fact.
* Similarly, you want to retain metrics for CI-runs without firing up a
prometheus server for all test runs.
* You want to execute specific system commands on a regular interval, because
the output might help you pin down the root cause of a problem quickly — both in
production and CI.

## Synopsis

The following is an example configuration that lets debugbunny scrape the
`/system_status`-endpoint of a locally running service every 15 seconds.
Further, the command `ss` is executed every 30 seconds, listing all open
TCP-sockets that are listening. Both scarpe targets time out after 250
milliseconds.

The design is modular. That is, scrape results can be processed in any way.
However, debugbunny comes with a `LogOutputWriter` that consumes the scrape
results and writes (compressed) log messages to a given writer.

```rust
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
            .action(Action::command_with_args(
                "/usr/bin/ss",
                vec!["-s", "-l"],
            ))
            .build(),
    );

    let stderr = stderr();
    let p = LogOutputWriter::new(stderr);
    let _debugbunny = DebugBunny::start_scraping(config.scrape_targets, p).await;
}
```
