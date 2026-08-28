//! Deterministic fixture generators.
//!
//! The centerpiece is the simulated agent fleet: N sandbox snapshots derived
//! from a few base templates with realistic mutation rates, in both raw and
//! gzip-compressed variants. This is the corpus behind the SRF benchmark
//! (M4 acceptance: SRF-S >= 5 through the engine).

use std::io::Write;

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

/// One generated fixture object.
#[derive(Debug, Clone)]
pub struct FixtureObject {
    pub key: String,
    pub bytes: Vec<u8>,
    pub content_type: &'static str,
}

/// Parameters for the simulated agent fleet.
#[derive(Debug, Clone)]
pub struct FleetSpec {
    pub seed: u64,
    /// Number of base templates (the spec fixture uses 3).
    pub templates: usize,
    /// Total snapshots derived from those templates.
    pub snapshots: usize,
    /// Size of each template in bytes.
    pub template_bytes: usize,
    /// Fraction of a snapshot's bytes mutated away from its template (~2%
    /// models a sandbox that ran for a while).
    pub mutation_rate: f64,
    /// Every Nth snapshot is stored gzip-compressed (0 = never).
    pub gzip_every: usize,
}

impl Default for FleetSpec {
    fn default() -> Self {
        Self {
            seed: 0x5041_5254,
            templates: 3,
            snapshots: 200,
            template_bytes: 8 * 1024 * 1024,
            mutation_rate: 0.02,
            gzip_every: 4,
        }
    }
}

/// Compressible-but-varied filler: long runs mixed with keyed pseudo-random
/// spans, roughly resembling a filesystem image (text, zeros, binaries).
fn fill_template(len: usize, seed: u64) -> Vec<u8> {
    let mut rng = StdRng::seed_from_u64(seed);
    let mut out = Vec::with_capacity(len);
    while out.len() < len {
        match rng.gen_range(0u8..4) {
            // zero runs (sparse files)
            0 => out.extend(std::iter::repeat_n(0u8, rng.gen_range(2048..16384))),
            // text-ish repeated words
            1 => {
                let word = b"agent sandbox state engine sparta ";
                for _ in 0..rng.gen_range(64..512) {
                    out.extend_from_slice(word);
                }
            }
            // structured counters (log files)
            2 => {
                let base: u64 = rng.gen();
                for i in 0..rng.gen_range(256u64..2048) {
                    out.extend_from_slice(format!("entry {} ok\n", base ^ i).as_bytes());
                }
            }
            // binary noise
            _ => {
                let n = rng.gen_range(4096..32768);
                let mut s: u64 = rng.gen();
                for _ in 0..n {
                    s ^= s << 13;
                    s ^= s >> 7;
                    s ^= s << 17;
                    out.push(s as u8);
                }
            }
        }
    }
    out.truncate(len);
    out
}

/// Derive a snapshot from a template and append a per-snapshot tail (new log
/// lines), like a sandbox that ran for a while.
///
/// Mutations are **localized**: real sandbox writes cluster in a handful of
/// files (a database, a log, a workspace dir), they don't sprinkle single
/// bytes across the whole image. The mutation budget is spent inside a few
/// contiguous hotspot regions so most spec-size (1 MiB avg) chunks survive
/// untouched — scattering the same budget uniformly would dirty every chunk
/// and model nothing real.
fn mutate(template: &[u8], rate: f64, seed: u64) -> Vec<u8> {
    let mut rng = StdRng::seed_from_u64(seed);
    let mut out = template.to_vec();
    let budget = (template.len() as f64 * rate) as usize;
    // Hotspots of up to ~256 KiB each.
    let hotspots = budget.div_ceil(256 * 1024).max(1);
    let per_hotspot = budget / hotspots;
    for _ in 0..hotspots {
        if per_hotspot == 0 || out.len() <= per_hotspot {
            break;
        }
        let at = rng.gen_range(0..out.len() - per_hotspot);
        for b in &mut out[at..at + per_hotspot] {
            *b = rng.gen();
        }
    }
    // Snapshot-unique tail: run logs.
    let tail_lines = rng.gen_range(50..200);
    for i in 0..tail_lines {
        out.extend_from_slice(
            format!(
                "run {:016x} step {} reward {:.4}\n",
                seed,
                i,
                rng.gen::<f64>()
            )
            .as_bytes(),
        );
    }
    out
}

pub fn gzip(raw: &[u8]) -> Vec<u8> {
    let mut enc = flate2::write::GzEncoder::new(
        Vec::with_capacity(raw.len() / 2),
        flate2::Compression::new(6),
    );
    enc.write_all(raw).expect("gzip fixture");
    enc.finish().expect("gzip fixture finish")
}

/// Generate the fleet, calling `sink` per object (keeps memory flat).
pub fn generate_fleet(spec: &FleetSpec, mut sink: impl FnMut(FixtureObject)) {
    let templates: Vec<Vec<u8>> = (0..spec.templates)
        .map(|t| fill_template(spec.template_bytes, spec.seed ^ (t as u64) << 32))
        .collect();
    for i in 0..spec.snapshots {
        let t = i % spec.templates;
        let raw = mutate(&templates[t], spec.mutation_rate, spec.seed ^ (i as u64));
        let gz = spec.gzip_every > 0 && i % spec.gzip_every == 0;
        if gz {
            sink(FixtureObject {
                key: format!("snapshots/tpl{t}/sandbox-{i:04}.tar.gz"),
                bytes: gzip(&raw),
                content_type: "application/gzip",
            });
        } else {
            sink(FixtureObject {
                key: format!("snapshots/tpl{t}/sandbox-{i:04}.tar"),
                bytes: raw,
                content_type: "application/x-tar",
            });
        }
    }
}

/// A small-object population for scanner throughput tests: `count` objects of
/// `size` bytes each, drawn from `families` distinct contents so there is
/// known redundancy.
pub fn generate_small_population(
    count: usize,
    size: usize,
    families: usize,
    seed: u64,
    mut sink: impl FnMut(FixtureObject),
) {
    let bodies: Vec<Vec<u8>> = (0..families.max(1))
        .map(|f| fill_template(size, seed ^ 0xF00D ^ (f as u64) << 24))
        .collect();
    for i in 0..count {
        let f = i % bodies.len();
        sink(FixtureObject {
            key: format!("objects/f{f:02}/obj-{i:06}.bin"),
            bytes: bodies[f].clone(),
            content_type: "application/octet-stream",
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fleet_is_deterministic() {
        let spec = FleetSpec {
            snapshots: 4,
            template_bytes: 256 * 1024,
            ..Default::default()
        };
        let mut a = Vec::new();
        generate_fleet(&spec, |o| a.push((o.key.clone(), o.bytes.len())));
        let mut b = Vec::new();
        generate_fleet(&spec, |o| b.push((o.key.clone(), o.bytes.len())));
        assert_eq!(a, b);
    }

    #[test]
    fn fleet_dedups_within_template_family() {
        // Pairwise sibling overlap understates redundancy (each sibling has
        // its own hotspot); the property the SRF depends on is family-level:
        // across many snapshots the template chunks dominate. Spec-scale
        // templates — with 1 MiB average chunks a small template has too few
        // chunks to measure.
        let spec = FleetSpec {
            snapshots: 24,
            templates: 1,
            template_bytes: 8 * 1024 * 1024,
            gzip_every: 0,
            ..Default::default()
        };
        let mut instances = 0usize;
        let mut distinct = std::collections::HashSet::new();
        generate_fleet(&spec, |o| {
            for s in sparta_core::chunker::chunk_spans(&o.bytes) {
                instances += 1;
                distinct.insert(s.hash);
            }
        });
        assert!(
            distinct.len() * 2 < instances,
            "expected >=2x chunk dedup across the family: {} distinct of {} instances",
            distinct.len(),
            instances
        );
    }

    #[test]
    fn gzip_variants_present() {
        let spec = FleetSpec {
            snapshots: 8,
            template_bytes: 128 * 1024,
            gzip_every: 4,
            ..Default::default()
        };
        let mut gz = 0;
        generate_fleet(&spec, |o| {
            if o.key.ends_with(".gz") {
                assert_eq!(&o.bytes[..2], &[0x1f, 0x8b]);
                gz += 1;
            }
        });
        assert_eq!(gz, 2);
    }
}
