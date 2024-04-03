use std::{str::FromStr, time::Duration};

use reqwest::{Method, Url};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_with::{serde_as, DurationSeconds};

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Config {
    pub scrape_targets: Vec<ScrapeTargetConfig>,
}

impl Config {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_target(&mut self, t: ScrapeTargetConfig) {
        self.scrape_targets.push(t);
    }
}

#[serde_as]
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ScrapeTargetConfig {
    /// todo(dsd): replace this with a string represention.
    #[serde_as(as = "DurationSeconds<u64>")]
    pub interval: Duration,
    pub timeout: Option<Duration>,
    pub action: Action,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(tag = "type")]
pub enum Action {
    Http {
        // xxx(dsd): potentially, we could use serde_with trick here, but I got
        // tired of fiddling around with it.
        #[serde(skip_serializing_if = "Option::is_none")]
        #[serde(serialize_with = "serialize_opt_method")]
        #[serde(deserialize_with = "deserialize_opt_method")]
        method: Option<Method>,
        url: Url,
    },
    Command {
        command: String,
        args: Vec<String>,
    },
}

impl Action {
    pub fn http(url: Url) -> Self {
        Self::Http { method: None, url }
    }

    pub fn http_with_method(url: Url, method: Method) -> Self {
        Self::Http {
            method: Some(method),
            url,
        }
    }

    pub fn command(command: String) -> Self {
        Self::Command {
            command,
            args: vec![],
        }
    }

    pub fn command_with_args<S: ToString, T: ToString>(command: S, args: Vec<T>) -> Self {
        let command = command.to_string();
        let args = args.iter().map(ToString::to_string).collect();
        Self::Command { command, args }
    }
}

#[derive(Default, Debug)]
pub struct ScrapeTargetBuilder {
    interval: Option<Duration>,
    timeout: Option<Duration>,
    action: Option<Action>,
}

impl ScrapeTargetBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn interval(mut self, d: Duration) -> Self {
        self.interval = Some(d);
        self
    }

    pub fn timeout(mut self, d: Duration) -> Self {
        self.timeout = Some(d);
        self
    }

    pub fn action(mut self, a: Action) -> Self {
        self.action = Some(a);
        self
    }

    pub fn build(self) -> ScrapeTargetConfig {
        ScrapeTargetConfig {
            interval: self.interval.expect("No interval set!"),
            timeout: self.timeout,
            action: self.action.expect("No action specified"),
        }
    }
}

fn serialize_opt_method<S>(v: &Option<Method>, s: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    match v {
        Some(m) => s.serialize_str(m.as_str()),
        None => s.serialize_none(),
    }
}

fn deserialize_opt_method<'de, D>(d: D) -> Result<Option<Method>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<&str>::deserialize(d).and_then(|s| match s {
        Some(s) => Ok(Some(Method::from_str(s).map_err(serde::de::Error::custom)?)),
        None => Ok(None),
    })
}
