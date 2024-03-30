use std::{borrow::Cow, future::Future, process::Output, sync::Arc};

use serde::Serialize;
use tokio::{io::AsyncWrite, sync::Mutex};
use zstd::zstd_safe::WriteBuf;

use crate::{
    config::ScrapeTargetConfig,
    scrape_target::{ScrapeOk, ScrapeResult},
};

pub trait ScrapeResultProcessor: Clone {
    fn process(
        &self,
        config: &ScrapeTargetConfig,
        result: ScrapeResult<ScrapeOk>,
    ) -> impl Future<Output = ()> + Send;
}

pub struct LogOutputWriter<T> {
    writer: Arc<Mutex<T>>,
}

impl<T> Clone for LogOutputWriter<T> {
    fn clone(&self) -> Self {
        Self {
            writer: self.writer.clone(),
        }
    }
}

impl<T> LogOutputWriter<T>
where
    T: AsyncWrite,
{
    pub fn new(writer: T) -> Self {
        Self {
            writer: Arc::new(Mutex::new(writer)),
        }
    }
}

impl<T> ScrapeResultProcessor for LogOutputWriter<T>
where
    T: AsyncWrite + Send,
{
    fn process(
        &self,
        config: &ScrapeTargetConfig,
        result: ScrapeResult<ScrapeOk>,
    ) -> impl Future<Output = ()> + Send {
        let writer = self.writer.clone();
        let config = config.clone();
        async move {
            // process results

            let _guard = writer.lock().await;
        }
    }
}

#[derive(Serialize)]
struct CommandMetaData {
    command: CommandSpec,
    exit_code: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    body_sha256: Option<String>,
}

#[derive(Serialize)]
struct CommandSpec {
    command: String,
    args: Vec<String>,
}

#[derive(Serialize)]
struct CommandBody {
    stdout: String,
    stderr: String,
}

impl From<Output> for CommandBody {
    fn from(value: Output) -> Self {
        let stdout = String::from_utf8_lossy(&value.stdout).to_string();
        let stderr = String::from_utf8_lossy(&value.stderr).to_string();
        Self { stdout, stderr }
    }
}
