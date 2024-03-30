use std::{borrow::Cow, io::Read};

use sha2::Digest;

const L: usize = 2922;

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
    data: ChunksData<'a>,
}

impl<'a> Chunks<'a> {
    pub fn from_split(v: Vec<Chunk<'a>>) -> Chunks<'a> {
        let mut hasher = sha2::Sha256::new();
        v.iter().for_each(|c| hasher.update(c.data.as_ref()));
        let id = (*hasher.finalize()).into();

        Self {
            id,
            data: ChunksData::Split(v),
        }
    }

    pub fn iter(&self) -> Box<dyn Iterator<Item = Chunk<'_>> + '_> {
        match &self.data {
            ChunksData::Split(vs) => Box::new(vs.iter().map(|c| Chunk {
                data: Cow::from(c.data.as_ref()),
                remaining: c.remaining,
            })),
            ChunksData::Owned(d) => {
                let total_len = d.len();
                Box::new(d.chunks(L).enumerate().map(move |(idx, w)| Chunk {
                    remaining: total_len - total_len.min(idx * L),
                    data: w.into(),
                }))
            }
        }
    }

    pub fn reader(&self) -> ChunksRead<'_> {
        ChunksRead {
            offset: 0,
            chunks: self,
        }
    }

    pub fn id(&self) -> Id {
        self.id.clone()
    }
}

impl From<Vec<u8>> for Chunks<'_> {
    fn from(v: Vec<u8>) -> Self {
        let id = (*sha2::Sha256::digest(&v)).into();
        Self {
            id,
            data: ChunksData::Owned(v),
        }
    }
}

pub struct ChunksRead<'a> {
    offset: usize,
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
                let idx = self.offset / L;
                let chunk_offset = self.offset - L * idx;
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

        let chunks1 = Chunks::from_split(chunks0.iter().map(|x| x.clone().into_owned()).collect());
        let id1 = chunks1.id();

        assert_eq!(id0, id1);
    }

    #[test]
    fn split_and_contiguous_have_same_content() {
        let data: Vec<_> = (0..7654).map(|x| (x % 256) as u8).collect();

        let chunks0 = Chunks::from(data);
        let mut buf0: Vec<u8> = vec![];
        std::io::copy(&mut chunks0.reader(), &mut buf0).unwrap();

        let chunks1 = Chunks::from_split(chunks0.iter().map(|x| x.clone().into_owned()).collect());
        let mut buf1: Vec<u8> = vec![];
        std::io::copy(&mut chunks1.reader(), &mut buf1).unwrap();

        assert_ne!(0, buf1.len());
        assert_eq!(buf0, buf1);
    }
}
