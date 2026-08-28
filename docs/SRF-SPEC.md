# SRF: The State Redundancy Factor — Measurement Specification v1

SRF quantifies how much redundancy exists in a corpus of stored objects, as
the ratio between the bytes a client believes it stored and the bytes actually
required after content-defined deduplication and compression:

```
SRF = logical_bytes / stored_bytes
```

An SRF of 6 means six logical bytes are represented by one stored byte. The
value of SRF as a metric depends entirely on its **reproducibility**: two
independent implementations measuring the same corpus must produce the same
number. This document fixes every parameter that affects the result.

## 1. Fixed parameters

| Parameter | Value |
|---|---|
| Chunking algorithm | FastCDC (2020 variant) |
| Minimum chunk size | 262,144 bytes (256 KiB) |
| Average (target) chunk size | 1,048,576 bytes (1 MiB) |
| Maximum chunk size | 4,194,304 bytes (4 MiB) |
| Chunk address | BLAKE3, 256-bit digest |
| Stored-size codec | zstd, level 3, single frame per chunk |
| Compression normalization | ON by default |
| MinHash sketch | 128 permutations, splitmix64-keyed over the first 8 bytes (LE) of each chunk digest; LSH banding 32 bands × 4 rows |

These values are deliberately conservative. Do not tune them per corpus: a
smaller SRF that is comparable beats a larger one that is not.

## 2. The measurement pipeline

For each object, in order:

### 2.1 Compression normalization

Objects stored compressed hide redundancy: the same payload gzipped at two
levels shares no bytes. Normalization recovers it, but only when byte-exact
reversal is proven:

1. Detect compression by magic bytes only (never file extension): gzip
   (`1f 8b`), zstd (`28 b5 2f fd`), xz (`fd 37 7a 58 5a 00`), zip (`50 4b`).
2. For gzip and zstd, decompress and derive a **recompression recipe**
   (gzip: verbatim header bytes + probed deflate level; zstd: probed level ×
   checksum flag × content-size flag).
3. **Verify**: recompress the raw payload with the recipe and compare the
   result byte-for-byte against the original stream.
4. On any mismatch — foreign encoder, unusual parameters, multi-member
   streams, integrity failure — the object is **opaque**: its original bytes
   are measured as-is. Opaque is always correct; normalization is an
   optimization that must prove itself per object.

The payload measured downstream is the raw payload for normalized objects and
the original bytes for opaque ones.

### 2.2 Boundary-aware chunking

If the payload is a recognized checkpoint container, cut points come from its
structure instead of content-defined boundaries:

- **safetensors**: the u64-LE header length prefix and each tensor's
  `data_offsets` begin (header JSON parsed strictly; parse failure = generic
  chunking).
- **zip** (PyTorch checkpoints): each central-directory entry's local header
  offset, plus the central directory offset.

Consecutive regions smaller than the minimum chunk size are merged (greedily,
in order — deterministic from the boundary layout); regions larger than the
maximum are sub-chunked with FastCDC restarted at the region start.

All other payloads are chunked with FastCDC at the fixed parameters.

### 2.3 Dedup and stored-size accounting

Each chunk is addressed by its BLAKE3 digest. Across the measured corpus:

```
logical_bytes  = Σ original object sizes (pre-normalization)
unique_chunks  = distinct chunk digests
stored_bytes   = Σ over unique chunks of min(len(zstd3(chunk)), len(chunk))
SRF            = logical_bytes / stored_bytes
```

The `min` clause reflects the raw-store fallback for incompressible chunks. A
reported SRF may additionally state `SRF_dedup` (logical / Σ unique raw chunk
bytes) to separate dedup gains from compression gains.

## 3. SRF-S: the sampled estimate

Measuring a fleet exhaustively is rarely practical. SRF-S is SRF measured on a
uniform random sample of objects:

- **Selection**: seeded uniform sampling over the enumerated population
  (partial Fisher–Yates; the seed is reported so the sample is reproducible).
- **Estimator**: SRF computed within the sample. Because duplicate chunks
  shared only with unsampled objects are not observed, **SRF-S is a
  conservative lower bound on fleet SRF** and must be reported as such.
- **Confidence interval**: exact leave-one-out jackknife over sampled
  objects. For object *i*, the leave-one-out ratio removes its logical bytes
  and the stored bytes of chunks whose every occurrence was in *i*; the 95%
  interval is `point ± 1.96·SE_jack`. (A naive bootstrap is invalid here:
  resampling with replacement duplicates objects, which dedup against
  themselves and inflate every resampled ratio.)
- **Reverification flag**: SRF-S is also computed with normalization
  disabled. If the two estimates differ by more than 3× in either direction,
  the report must be prominently marked as requiring reverification.
- **Addressable SRF-S**: the same estimator with operator-excluded objects
  (DR copies, replication targets — by key prefix/glob or object tag) removed
  from both numerator and denominator.

Large objects may be fingerprinted on a prefix window under a byte budget;
windowed objects skip normalization (a partial compressed stream cannot be
verified) and the report must disclose the count.

## 4. VSRR: the Verified Savings Run Rate

SRF describes data; VSRR describes money, and only ever from metered actuals:

```
net_savings(window) = storage_cost(logical GiB-months) − storage_cost(physical GiB-months)
                      − engine request costs − engine compute costs
VSRR = net_savings(trailing 30 days) × 12.1667
```

- Storage is a **time integral** of level gauges (step-hold between gauge
  records), not a sum of bytes written.
- Engine overheads (pack/manifest requests by class, measured engine CPU
  time) are subtracted; savings are NET by construction.
- Every input comes from an append-only, hash-chained meter log that
  revalidates end to end (`sparta-meter verify`). Figures derived from
  shadow-mode projections must be labeled as projections.

## 5. Conformance

An implementation conforms if, for any corpus:

1. It reproduces this pipeline exactly (parameters of §1, rules of §2).
2. Full-corpus SRF matches a reference implementation to within floating
   point error.
3. Every normalized object round-trips byte-exactly through its recipe.
4. SRF-S reports include: seed, sample size, coverage fractions, CI method,
   the no-normalization comparison, and the windowed-object count.

The reference implementation is `sparta-core` (chunking, normalization,
accounting) and `sparta-scanner` (SRF-S estimation) in this repository.
