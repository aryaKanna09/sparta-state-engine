//! Shadow-mode taps: observe streamed bytes without ever being able to fail
//! the customer's request.
//!
//! The tap wraps a request or response body stream and forwards every frame
//! untouched. All observation work (counting, chunking, compression estimates)
//! happens behind `catch_unwind`; the first panic or error disarms the tap for
//! the rest of the stream and bytes keep flowing. Invariant 0 by construction.

use std::panic::AssertUnwindSafe;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use bytes::Bytes;
use futures::Stream;
use s3s::dto::StreamingBlob;
use s3s::stream::{ByteStream, DynByteStream, RemainingLength};
use s3s::StdError;
use tracing::warn;

use sparta_core::hash::ChunkHash;
use sparta_meter::log::MeterWriter;
use sparta_meter::record::{Mode, RecordBody};

use crate::chunkstream::IncrementalChunker;
use crate::db::Db;

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Observation state fed as bytes stream through.
struct TapState {
    chunker: Option<IncrementalChunker>,
    observed: Vec<(ChunkHash, u64, u64)>,
    bytes: u64,
    db: Arc<Db>,
    meter: Arc<MeterWriter>,
    op: &'static str,
    bucket: String,
    /// Counter names to bump with `bytes` at end of stream.
    byte_counter: &'static str,
    record_chunks: bool,
    disarmed: bool,
    panic_fault: bool,
    cpu_ns: u64,
}

impl TapState {
    fn feed(&mut self, data: &[u8]) {
        // Fault-suite hook: prove a panicking chunker cannot fail a request.
        // Inert unless the env var is set (test processes only).
        if self.panic_fault {
            panic!("injected tap panic (SPARTA_FAULT_PANIC_TAP)");
        }
        self.bytes += data.len() as u64;
        if let Some(chunker) = &mut self.chunker {
            let t0 = std::time::Instant::now();
            for c in chunker.feed(data) {
                self.observed.push((c.hash, c.raw_len, 0));
            }
            self.cpu_ns += t0.elapsed().as_nanos() as u64;
        }
    }

    fn finish(&mut self) {
        if let Some(mut chunker) = self.chunker.take() {
            let t0 = std::time::Instant::now();
            for c in chunker.finish() {
                self.observed.push((c.hash, c.raw_len, 0));
            }
            self.cpu_ns += t0.elapsed().as_nanos() as u64;
        }
        // Stored-size estimates: shadow mode has already forwarded the bytes,
        // so we only know chunk hashes and lengths here. Estimate stored size
        // as raw_len/2 would be dishonest; instead record raw_len and let the
        // conservative ratio hold: stored_est = raw_len (no compression
        // credit). The scanner (with real bytes) reports the compressed view.
        for (_, raw, stored) in self.observed.iter_mut() {
            *stored = *raw;
        }
        let _ = self.db.bump(self.byte_counter, self.bytes);
        if self.record_chunks {
            if let Err(e) = self.db.record_shadow_stream(self.bytes, &self.observed) {
                warn!(error = %e, "shadow stream record failed (stats only, request unaffected)");
            }
        }
        // Meter record: shadow mode is pure passthrough, physical == logical.
        if let Err(e) = self.meter.append(
            now_ms(),
            Mode::Shadow,
            RecordBody::Op {
                op: self.op.to_string(),
                bucket: std::mem::take(&mut self.bucket),
                logical_bytes: self.bytes,
                physical_bytes: self.bytes,
                engine_put_requests: 0,
                engine_get_requests: 0,
                engine_bytes: 0,
                engine_cpu_us: self.cpu_ns / 1000,
            },
        ) {
            warn!(error = %e, "meter append failed (metering only, request unaffected)");
        }
        self.observed.clear();
    }
}

/// A body stream wrapper that forwards frames and observes them fallibly.
pub struct TapStream {
    inner: DynByteStream,
    state: TapState,
    finished: bool,
}

impl TapStream {
    fn guard<F: FnOnce(&mut TapState)>(state: &mut TapState, f: F) {
        if state.disarmed {
            return;
        }
        if std::panic::catch_unwind(AssertUnwindSafe(|| f(state))).is_err() {
            state.disarmed = true;
            warn!("shadow tap panicked; disarmed for the rest of this stream");
        }
    }
}

impl Stream for TapStream {
    type Item = Result<Bytes, StdError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = &mut *self;
        match Pin::new(&mut this.inner).poll_next(cx) {
            Poll::Ready(Some(Ok(bytes))) => {
                Self::guard(&mut this.state, |s| s.feed(&bytes));
                Poll::Ready(Some(Ok(bytes)))
            }
            Poll::Ready(None) => {
                if !this.finished {
                    this.finished = true;
                    Self::guard(&mut this.state, |s| s.finish());
                }
                Poll::Ready(None)
            }
            other => other,
        }
    }
}

impl ByteStream for TapStream {
    fn remaining_length(&self) -> RemainingLength {
        self.inner.remaining_length()
    }
}

impl Drop for TapStream {
    fn drop(&mut self) {
        // HTTP clients typically stop polling a body once content-length
        // bytes have been read, so `Ready(None)` may never be observed; the
        // drop after send completion is the reliable end-of-stream signal.
        if !self.finished {
            self.finished = true;
            let state = &mut self.state;
            if std::panic::catch_unwind(AssertUnwindSafe(|| state.finish())).is_err() {
                warn!("shadow tap finalization panicked; stats dropped for this stream");
            }
        }
    }
}

/// Wrap a request/response body so its bytes are observed in shadow mode.
///
/// `chunk` enables CDC fingerprinting (PUT/UploadPart); plain byte counting
/// otherwise (GET responses).
#[allow(clippy::too_many_arguments)]
pub fn tap_blob(
    blob: StreamingBlob,
    db: Arc<Db>,
    meter: Arc<MeterWriter>,
    op: &'static str,
    bucket: String,
    byte_counter: &'static str,
    chunk: bool,
) -> StreamingBlob {
    let stream = TapStream {
        inner: blob.into(),
        state: TapState {
            chunker: chunk.then(IncrementalChunker::new),
            observed: Vec::new(),
            bytes: 0,
            db,
            meter,
            op,
            bucket,
            byte_counter,
            record_chunks: chunk,
            disarmed: false,
            panic_fault: std::env::var("SPARTA_FAULT_PANIC_TAP").is_ok_and(|v| v == "1"),
            cpu_ns: 0,
        },
        finished: false,
    };
    StreamingBlob::new(stream)
}
