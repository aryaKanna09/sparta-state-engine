//! Incremental content-defined chunking for streamed bodies.
//!
//! FastCDC cut decisions depend only on bytes up to the cut plus a bounded
//! look-ahead (the max chunk size), and the chunker state resets at every cut.
//! So chunking is restartable at cut boundaries: we buffer streamed bytes,
//! emit every chunk whose cut cannot be affected by future bytes (its end is
//! at least `max_size` from the buffer end), and drain them. Memory stays
//! bounded by a few max-chunk sizes regardless of object size, and the result
//! is byte-identical to one-shot chunking of the whole object.

use sparta_core::chunker::chunk_spans;
use sparta_core::hash::ChunkHash;
use sparta_core::spec;

pub struct IncrementalChunker {
    buf: Vec<u8>,
    /// Total bytes fed so far.
    pub total_len: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EmittedChunk {
    pub hash: ChunkHash,
    pub raw_len: u64,
}

impl IncrementalChunker {
    pub fn new() -> Self {
        Self {
            buf: Vec::with_capacity(2 * spec::CDC_MAX_SIZE as usize),
            total_len: 0,
        }
    }

    /// Feed streamed bytes; returns chunks whose boundaries are now final.
    pub fn feed(&mut self, data: &[u8]) -> Vec<EmittedChunk> {
        self.total_len += data.len() as u64;
        self.buf.extend_from_slice(data);
        let safe_end = self.buf.len().saturating_sub(spec::CDC_MAX_SIZE as usize);
        if safe_end < spec::CDC_MIN_SIZE as usize {
            return Vec::new();
        }
        let mut out = Vec::new();
        let mut consumed = 0usize;
        for span in chunk_spans(&self.buf) {
            // A cut at `offset + length` is final iff there are still
            // `max_size` bytes after it in the buffer: FastCDC would place the
            // same cut no matter what arrives later.
            if span.offset + span.length <= safe_end {
                out.push(EmittedChunk {
                    hash: span.hash,
                    raw_len: span.length as u64,
                });
                consumed = span.offset + span.length;
            } else {
                break;
            }
        }
        if consumed > 0 {
            self.buf.drain(..consumed);
        }
        out
    }

    /// Flush at end of stream: chunk whatever remains.
    pub fn finish(&mut self) -> Vec<EmittedChunk> {
        let out = chunk_spans(&self.buf)
            .into_iter()
            .map(|s| EmittedChunk {
                hash: s.hash,
                raw_len: s.length as u64,
            })
            .collect();
        self.buf.clear();
        out
    }
}

impl Default for IncrementalChunker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pseudo_random(len: usize, seed: u64) -> Vec<u8> {
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

    fn run_incremental(data: &[u8], feed_size: usize) -> Vec<EmittedChunk> {
        let mut c = IncrementalChunker::new();
        let mut out = Vec::new();
        for part in data.chunks(feed_size) {
            out.extend(c.feed(part));
        }
        out.extend(c.finish());
        out
    }

    #[test]
    fn matches_oneshot_chunking() {
        let data = pseudo_random(23 * 1024 * 1024 + 517, 99);
        let oneshot: Vec<EmittedChunk> = chunk_spans(&data)
            .into_iter()
            .map(|s| EmittedChunk {
                hash: s.hash,
                raw_len: s.length as u64,
            })
            .collect();
        for feed in [4096, 65536, 1 << 20, 8 << 20] {
            let inc = run_incremental(&data, feed);
            assert_eq!(inc, oneshot, "feed size {feed} diverged from one-shot");
        }
    }

    #[test]
    fn bounded_memory() {
        let mut c = IncrementalChunker::new();
        let block = pseudo_random(1 << 20, 3);
        for _ in 0..64 {
            let _ = c.feed(&block);
            assert!(
                c.buf.len() <= 2 * spec::CDC_MAX_SIZE as usize + (1 << 20),
                "buffer grew to {}",
                c.buf.len()
            );
        }
    }

    #[test]
    fn empty_and_tiny_streams() {
        let mut c = IncrementalChunker::new();
        assert!(c.feed(b"").is_empty());
        assert!(c.finish().is_empty());

        let mut c = IncrementalChunker::new();
        assert!(c.feed(b"tiny").is_empty());
        let fin = c.finish();
        assert_eq!(fin.len(), 1);
        assert_eq!(fin[0].raw_len, 4);
        assert_eq!(fin[0].hash, ChunkHash::of(b"tiny"));
    }
}
