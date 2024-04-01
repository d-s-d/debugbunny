use std::{borrow::Cow, future::Future, io::Cursor, process::Output, sync::Arc};

use http::StatusCode;
use serde::{Deserialize, Serialize};
use serde_with::{serde_as, DisplayFromStr};
use tokio::{io::AsyncWrite, sync::Mutex};

use crate::{
    chunkify::{Chunks, Id, DEFAULT_CHUNK_SIZE},
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

/// Serialize the result of a scrape call as JSON-object and write it to the
/// wrapped writer.
///
/// An instance of [LogOutputWriter] is `Send + Sync + Clone`, so can (and
/// should) be shared between threads. Writes to the wrapped `WriteAsync` are
/// fully serialized.
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
    T: AsyncWrite + Send,
{
    pub fn new(writer: T) -> Self {
        Self {
            writer: Arc::new(Mutex::new(writer)),
        }
    }
}

impl<T> ScrapeResultProcessor for LogOutputWriter<T>
where
    T: AsyncWrite + Unpin + Send + 'static
{
    fn process(
        &self,
        config: &ScrapeTargetConfig,
        result: ScrapeResult<ScrapeOk>,
    ) -> impl Future<Output = ()> + Send {
        let writer = self.writer.clone();
        let config = config.clone();
        async move {
            // As we are performing compression here, we dispatch the
            // computation to a background thread in order not to block the
            // io-thread.
            let (mut meta, chunks) = tokio::task::spawn_blocking(move || {
                let (r, c) = ScrapeResultRepr::from_scrape_result(result);
                let meta = ScrapeCall { target_config: config, result: r };    
                let meta = Cursor::new(serde_json::to_vec(&meta).expect("can't fail"));
                (meta, c)
            }).await.expect("Could not join blocking code!");

            {
                let mut guard = writer.lock().await;
                let _ = tokio::io::copy(&mut meta, &mut *guard).await;

                if let Some(chunks) = chunks {
                    let id = chunks.id();
                    for c in chunks.iter() {
                        #[derive(Serialize)]
                        struct ChunkRepr<'a> {
                            id: Id,
                            remaining: usize,
                            data: Cow<'a, [u8]>
                        }

                        let c = ChunkRepr {
                            id: id,
                            remaining: c.remaining,
                            data: c.data
                        };

                        let mut chunk_json = Cursor::new(serde_json::to_vec(&c).expect("can't fail"));
                        let _ = tokio::io::copy(&mut chunk_json, &mut *guard).await;
                    }
                }
            }
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct ScrapeCall {
    target_config: ScrapeTargetConfig,
    result: ScrapeResultRepr,
}

// Boilerplate for serialization of scrape results.

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type")]
pub enum ScrapeResultRepr {
    Success(ScrapeOkRepr),
    Error { message: String },
}

impl ScrapeResultRepr {
    fn from_scrape_result(v: ScrapeResult<ScrapeOk>) -> (Self, Option<Chunks<'static>>) {
        match v {
            Ok(success) => {
                let (r, c) = Self::scrape_ok_to_meta(success);
                (Self::Success(r), Some(c))
            }
            Err(e) => (
                Self::Error {
                    message: format!("{e:?}"),
                },
                None,
            ),
        }
    }

    /// Transform successful scrape call to serializable objects.
    fn scrape_ok_to_meta(ok: ScrapeOk) -> (ScrapeOkRepr, Chunks<'static>) {
        match ok {
            ScrapeOk::HttpResponse(r) => {
                let (parts, body) = r.into_parts();
                // As we perform only in-memory computations here, we simply unwrap
                // the error and fail hard.
                let compressed =
                    zstd::encode_all(Cursor::new(body), 10).expect("zstd compression failed");
                let chunks = Chunks::new(compressed, DEFAULT_CHUNK_SIZE);
                (
                    ScrapeOkRepr::Http {
                        status: parts.status,
                        body_sha256: chunks.id(),
                    },
                    chunks,
                )
            }
            ScrapeOk::CommandResponse(c) => {
                let exit_code = c.status.code().unwrap_or(1);
                let cbody: CommandBody = c.into();
                let cbody = serde_json::to_vec(&cbody).expect("json encoding failed.");
                // As we perform only in-memory computations here, we simply unwrap
                // the error and fail hard.
                let compressed =
                    zstd::encode_all(Cursor::new(cbody), 10).expect("zstd compression failed");
                let chunks = Chunks::new(compressed, DEFAULT_CHUNK_SIZE);
                (
                    ScrapeOkRepr::Command {
                        exit_code,
                        body_sha256: chunks.id(),
                    },
                    chunks,
                )
            }
        }
    }
}

#[serde_as]
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[serde(tag = "type")]
pub enum ScrapeOkRepr {
    Http {
        #[serde_as(as = "DisplayFromStr")]
        status: StatusCode,
        body_sha256: Id,
    },
    Command {
        exit_code: i32,
        body_sha256: Id,
    },
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
