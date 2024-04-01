/// A helper construct to chunk up a contiguous byte array or treat a vector of
/// chunks as a single contiguous byte string. In either case, additional
/// allocations are avoided.
use std::{borrow::Cow, io::Read};

use serde::{Deserialize, Serialize};
use serde_with::serde_as;
use sha2::Digest;
use thiserror::Error;

/// The default chunk size is chosen with logging in mind: We assume that log
/// messages are text-based and may have a maximum size of 4096 bytes. This
/// assumption is informed by the limits systemd-services, such as gatewayd,
/// place on the size of log-message when encoding journald-entries as
/// [json](https://github.com/systemd/systemd/blob/3799fa803efb04cdd1f1b239c6c64803fe85d13a/src/shared/logs-show.c#L46).
///
/// Assuming that each chunk is serialized to a log message and the chunk data
/// is base64 encoded, the choice of the default chunk size comes about as
/// follows: The Base64-encoding represents 3 bytes of input data in 4 bytes.
/// Thus, a base64-encoded chunk of length 2922 will end up having a length of
/// `2922*4/3 == 3896` bytes. Further assuming that the chunk is encoded as part
/// of a json-object, this leaves 200 bytes for additional metadata.
pub const DEFAULT_CHUNK_SIZE: usize = 2922;

#[serde_as]
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Id(#[serde_as(as = "serde_with::hex::Hex")] [u8; 32]);

impl From<[u8; 32]> for Id {
    fn from(value: [u8; 32]) -> Self {
        Self(value)
    }
}

impl From<&[u8]> for Id {
    fn from(value: &[u8]) -> Self {
        let mut id = [0u8; 32];
        id[0..].copy_from_slice(&value[..32]);
        Self(id)
    }
}

pub struct Chunks<'a> {
    id: Id,
    chunk_size: usize,
    data: ChunksData<'a>,
}

impl<'a> Chunks<'a> {
    pub fn new<T: Into<Vec<u8>>>(data: T, chunk_size: usize) -> Self {
        let data: Vec<u8> = data.into();
        let id = (*sha2::Sha256::digest(&data)).into();
        Self {
            id,
            chunk_size,
            data: ChunksData::Contiguous(data),
        }
    }

    /// Create a Chunks-object from chunks. The `remaining`-field will be
    /// overriden with the actual remaining bytes.
    pub fn from_chunks(mut v: Vec<Chunk<'a>>) -> Result<Chunks<'a>, ChunksError>
    {
        let mut r_iter = v.iter().rev();
        let chunk_size = v.first().map(|c| c.data.len()).unwrap_or(0);
        if r_iter.clone().skip(1).any(|c| c.data.len() != chunk_size) {
            return Err(ChunksError::ChunksSizeMismatch);
        }

        let mut last_size = v.last().map(|c| c.data.len()).unwrap_or(0);
        r_iter.try_for_each(|c| {
            if c.remaining != last_size {
                return Err(ChunksError::InvalidRemainingValue);
            }
            last_size += chunk_size;
            Ok(())
        })?;

        let mut hasher = sha2::Sha256::new();
        v.iter().for_each(|c| hasher.update(c.data.as_ref()));
        let id = (*hasher.finalize()).into();

        Ok(Self {
            id,
            chunk_size,
            data: ChunksData::Chunked(v),
        })
    }

    pub fn iter(&self) -> Box<dyn Iterator<Item = Chunk<'_>> + '_ + Send> {
        match &self.data {
            ChunksData::Chunked(vs) => Box::new(vs.iter().map(|c| Chunk {
                data: Cow::from(c.data.as_ref()),
                remaining: c.remaining,
            })),
            ChunksData::Contiguous(d) => {
                let total_len = d.len();
                Box::new(
                    d.chunks(self.chunk_size)
                        .enumerate()
                        .map(move |(idx, w)| Chunk {
                            remaining: total_len - total_len.min(idx * self.chunk_size),
                            data: w.into(),
                        }),
                )
            }
        }
    }

    pub fn reader(&self) -> ChunksRead<'_> {
        ChunksRead {
            offset: 0,
            chunk_size: self.chunk_size,
            chunks: self,
        }
    }

    pub fn id(&self) -> Id {
        self.id.clone()
    }

    pub fn chunk_size(&self) -> usize {
        self.chunk_size
    }
}

#[derive(Debug, Error)]
pub enum ChunksError {
    #[error("All but the last chunk must have the same length.")]
    ChunksSizeMismatch,
    #[error("At least one value of a 'remaining'-field is invalid.")]
    InvalidRemainingValue,
}

impl From<Vec<u8>> for Chunks<'_> {
    fn from(v: Vec<u8>) -> Self {
        Self::new(v, DEFAULT_CHUNK_SIZE)
    }
}

pub struct ChunksRead<'a> {
    offset: usize,
    chunk_size: usize,
    chunks: &'a Chunks<'a>,
}

impl<'a> Read for ChunksRead<'a> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let buf_len = buf.len();
        match &self.chunks.data {
            ChunksData::Contiguous(d) => {
                let actual_len = (d.len() - self.offset).min(buf_len);
                buf[0..actual_len].clone_from_slice(&d[self.offset..(self.offset + actual_len)]);
                self.offset += actual_len;
                Ok(actual_len)
            }
            ChunksData::Chunked(c) => {
                if c.is_empty() {
                    return Ok(0);
                }
                let idx = self.offset / self.chunk_size;
                let chunk_offset = self.offset - self.chunk_size * idx;
                let src = c[idx].data.as_ref();
                assert!(src.len() >= chunk_offset);
                let actual_len = (src.len() - chunk_offset).min(buf_len);
                buf[0..actual_len]
                    .clone_from_slice(&src[chunk_offset..(chunk_offset + actual_len)]);
                self.offset += actual_len;
                Ok(actual_len)
            }
        }
    }
}

enum ChunksData<'a> {
    Contiguous(Vec<u8>),
    Chunked(Vec<Chunk<'a>>),
}

#[derive(Debug, Clone)]
pub struct Chunk<'a> {
    pub remaining: usize,
    pub data: Cow<'a, [u8]>,
}

impl<'a> Chunk<'a> {
    pub fn into_owned(self) -> Chunk<'static> {
        Chunk {
            data: Cow::from(self.data.into_owned()),
            remaining: self.remaining,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn split_and_contiguous_has_same_id() {
        let data: Vec<_> = (0..7654).map(|x| (x % 256) as u8).collect();

        let chunks0 = Chunks::from(data);
        let id0 = chunks0.id();
        assert_ne!(id0, Id::from([0u8; 32]));

        let chunks1 =
            Chunks::from_chunks(chunks0.iter().map(|x| x.clone().into_owned()).collect()).unwrap();
        let id1 = chunks1.id();

        assert_eq!(id0, id1);
    }

    #[test]
    fn split_and_contiguous_have_same_content() {
        let data: Vec<_> = (0..7654).map(|x| (x % 256) as u8).collect();

        let chunks0 = Chunks::from(data);
        let mut buf0: Vec<u8> = vec![];
        std::io::copy(&mut chunks0.reader(), &mut buf0).unwrap();

        let chunks1 =
            Chunks::from_chunks(chunks0.iter().map(|x| x.clone().into_owned()).collect()).unwrap();
        let mut buf1: Vec<u8> = vec![];
        std::io::copy(&mut chunks1.reader(), &mut buf1).unwrap();

        assert_ne!(0, buf1.len());
        assert_eq!(buf0, buf1);
    }
}
