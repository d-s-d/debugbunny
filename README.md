# 🐰 debugbunny 🛠️

A small toolbox to scrape HTTP-endpoints and commands on a regular basis and
publish the results as log messages.

## Overview

Let's say you operate a service and you want to assess part of the system state
after the fact — debugbunny is here to help you. It scrapes previously specified
_scrape targets_ on a regular interval and emits the results as log messages (if
so configured).

As of now, a scrape target is either an HTTP-endpoint or a shell command.

## Example configuration


```rust
let mut config = Config::new();
let half_min = Duration::from_secs(30);
let quarter_min = half_sec / 2;
let url = Url::parse("http://localhost:8080/system_status").unwrap();
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
```
