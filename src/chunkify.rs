/// A helper construct to chunk up a contiguous byte array or treat a vector of
/// chunks as a single contiguous byte string. In either case, the construct
/// avoids additional allocations.
use std::{borrow::Cow, io::Read};

use sha2::Digest;
use thiserror::Error;

/// The default chunks size is assume
pub const DEFAULT_CHUNK_SIZE: usize = 2922;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Id([u8; 32]);

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
    pub fn new<T: AsRef<[u8]>>(data: T, chunk_size: usize) -> Self {
        let id = (*sha2::Sha256::digest(data.as_ref())).into();
        Self {
            id,
            chunk_size,
            data: ChunksData::Owned(data.as_ref().to_owned()),
        }
    }

    pub fn from_split(v: Vec<Chunk<'a>>) -> Result<Chunks<'a>, ChunksError> {
        let chunk_size = v.first().map(|c| c.data.len()).unwrap_or(0);
        if v.iter().rev().skip(1).any(|c| c.data.len() != chunk_size) {
            return Err(ChunksError::ChunksSizeMismatch);
        }

        let mut hasher = sha2::Sha256::new();
        v.iter().for_each(|c| hasher.update(c.data.as_ref()));
        let id = (*hasher.finalize()).into();

        Ok(Self {
            id,
            chunk_size,
            data: ChunksData::Split(v),
        })
    }

    pub fn iter(&self) -> Box<dyn Iterator<Item = Chunk<'_>> + '_> {
        match &self.data {
            ChunksData::Split(vs) => Box::new(vs.iter().map(|c| Chunk {
                data: Cow::from(c.data.as_ref()),
                remaining: c.remaining,
            })),
            ChunksData::Owned(d) => {
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
            ChunksData::Owned(d) => {
                let actual_len = (d.len() - self.offset).min(buf_len);
                buf[0..actual_len].clone_from_slice(&d[self.offset..(self.offset + actual_len)]);
                self.offset += actual_len;
                Ok(actual_len)
            }
            ChunksData::Split(c) => {
                let idx = self.offset / self.chunk_size;
                let chunk_offset = self.offset - self.chunk_size * idx;
                let src = c[idx].data.as_ref();
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
    Owned(Vec<u8>),
    Split(Vec<Chunk<'a>>),
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
            Chunks::from_split(chunks0.iter().map(|x| x.clone().into_owned()).collect()).unwrap();
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
            Chunks::from_split(chunks0.iter().map(|x| x.clone().into_owned()).collect()).unwrap();
        let mut buf1: Vec<u8> = vec![];
        std::io::copy(&mut chunks1.reader(), &mut buf1).unwrap();

        assert_ne!(0, buf1.len());
        assert_eq!(buf0, buf1);
    }
}
