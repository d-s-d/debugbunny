use std::time::Duration;

use reqwest::{Method, Url};

pub struct Config {
    pub scrape_targets: Vec<ScrapeTargetConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScrapeTargetConfig {
    interval: Duration,
    timeout: Option<Duration>,
    action: Action,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Http { method: Option<Method>, url: Url },
    Command { command: String, args: Vec<String> },
}
