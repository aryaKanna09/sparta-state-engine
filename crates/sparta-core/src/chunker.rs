//! Content-defined chunking at the SRF spec parameters.
//!
//! FastCDC (v2020 variant) with min/avg/max = 256 KiB / 1 MiB / 4 MiB. These
//! parameters are part of the published SRF spec; the SRF number is only
//! reproducible across implementations if everyone chunks identically.

use std::io::Read;

use fastcdc::v2020::{FastCDC, StreamCDC};

use crate::hash::ChunkHash;
use crate::{spec, CoreError};

/// A chunk boundary within a byte slice, plus its BLAKE3 address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkSpan {
    pub offset: usize,
    pub length: usize,
    pub hash: ChunkHash,
}

/// Chunk an in-memory payload. Returns spans in order; concatenating
/// `data[offset..offset+length]` over all spans reproduces `data` exactly.
///
/// Empty input yields no chunks.
pub fn chunk_spans(data: &[u8]) -> Vec<ChunkSpan> {
    FastCDC::new(
        data,
        spec::CDC_MIN_SIZE,
        spec::CDC_AVG_SIZE,
        spec::CDC_MAX_SIZE,
    )
    .map(|c| ChunkSpan {
        offset: c.offset,
        length: c.length,
        hash: ChunkHash::of(&data[c.offset..c.offset + c.length]),
    })
    .collect()
}

/// An owned chunk produced from a streaming source.
#[derive(Debug, Clone)]
pub struct OwnedChunk {
    pub offset: u64,
    pub data: Vec<u8>,
    pub hash: ChunkHash,
}

impl OwnedChunk {
    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}

/// Streaming chunker over any `Read`, at spec parameters. Yields chunks in
/// stream order without buffering the whole object.
pub struct StreamChunker<R: Read> {
    inner: StreamCDC<R>,
}

impl<R: Read> StreamChunker<R> {
    pub fn new(reader: R) -> Self {
        Self {
            inner: StreamCDC::new(
                reader,
                spec::CDC_MIN_SIZE,
                spec::CDC_AVG_SIZE,
                spec::CDC_MAX_SIZE,
            ),
        }
    }
}

impl<R: Read> Iterator for StreamChunker<R> {
    type Item = Result<OwnedChunk, CoreError>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.inner.next()? {
            Ok(cd) => {
                let hash = ChunkHash::of(&cd.data);
                Some(Ok(OwnedChunk {
                    offset: cd.offset,
                    data: cd.data,
                    hash,
                }))
            }
            Err(e) => Some(Err(CoreError::Io(std::io::Error::other(e.to_string())))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pseudo_random(len: usize, seed: u64) -> Vec<u8> {
        // Deterministic xorshift fill; incompressible-ish data exercises real
        // chunk boundaries.
        let mut out = Vec::with_capacity(len);
        let mut s = seed | 1;
        while out.len() < len {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            out.extend_from_slice(&s.to_le_bytes());
        }
        out.truncate(len);
        out
    }

    #[test]
    fn empty_input_no_chunks() {
        assert!(chunk_spans(&[]).is_empty());
    }

    #[test]
    fn spans_cover_input_exactly() {
        let data = pseudo_random(10 * 1024 * 1024 + 12345, 42);
        let spans = chunk_spans(&data);
        assert!(!spans.is_empty());
        let mut pos = 0usize;
        for s in &spans {
            assert_eq!(s.offset, pos, "spans must be contiguous");
            pos += s.length;
        }
        assert_eq!(pos, data.len(), "spans must cover the whole input");
    }

    #[test]
    fn size_bounds_respected() {
        let data = pseudo_random(20 * 1024 * 1024, 7);
        let spans = chunk_spans(&data);
        for (i, s) in spans.iter().enumerate() {
            assert!(
                s.length <= spec::CDC_MAX_SIZE as usize,
                "chunk {i} exceeds max size"
            );
            if i + 1 < spans.len() {
                assert!(
                    s.length >= spec::CDC_MIN_SIZE as usize,
                    "non-final chunk {i} below min size"
                );
            }
        }
    }

    #[test]
    fn chunking_is_deterministic() {
        let data = pseudo_random(5 * 1024 * 1024, 99);
        assert_eq!(chunk_spans(&data), chunk_spans(&data));
    }

    #[test]
    fn insertion_shifts_few_chunks() {
        // The point of CDC: a small insertion must not re-chunk the tail.
        let base = pseudo_random(16 * 1024 * 1024, 5);
        let mut edited = base.clone();
        edited.splice(1_000_000..1_000_000, [0xAAu8; 64].iter().copied());

        let a: std::collections::HashSet<_> =
            chunk_spans(&base).into_iter().map(|s| s.hash).collect();
        let b: Vec<_> = chunk_spans(&edited);
        let changed = b.iter().filter(|s| !a.contains(&s.hash)).count();
        assert!(
            changed <= 4,
            "64-byte insertion changed {changed} of {} chunks",
            b.len()
        );
    }

    #[test]
    fn stream_matches_slice_chunking() {
        let data = pseudo_random(9 * 1024 * 1024 + 3, 1234);
        let slice_spans = chunk_spans(&data);
        let stream_chunks: Vec<_> = StreamChunker::new(&data[..])
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(slice_spans.len(), stream_chunks.len());
        for (a, b) in slice_spans.iter().zip(&stream_chunks) {
            assert_eq!(a.hash, b.hash);
            assert_eq!(a.length, b.data.len());
            assert_eq!(a.offset as u64, b.offset);
        }
    }
}
