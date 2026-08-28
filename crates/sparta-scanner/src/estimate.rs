//! SRF-S estimation from sampled fingerprints: point estimate, jackknife
//! confidence interval, grouped breakdowns, and redundancy clusters.

use std::collections::HashMap;

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use serde::Serialize;

use sparta_core::minhash::MinHashSketch;
use sparta_core::srf::{DedupAccumulator, DedupStats};

use crate::types::ObjectFingerprint;

/// Which chunk view of each fingerprint to accumulate.
#[derive(Clone, Copy, PartialEq)]
pub enum View {
    /// Normalized payload chunks (the spec pipeline).
    Normalized,
    /// Original-bytes chunks, as if normalization were off.
    Opaque,
}

/// Build an accumulator over `fps`, optionally filtering excluded objects.
pub fn accumulate(fps: &[&ObjectFingerprint], view: View, with_groups: bool) -> DedupAccumulator {
    let mut acc = DedupAccumulator::new();
    for fp in fps {
        let chunks: Vec<_> = chunk_view(fp, view)
            .iter()
            .map(|c| (c.hash, c.raw_len))
            .collect();
        let stored: HashMap<_, _> = chunk_view(fp, view)
            .iter()
            .map(|c| (c.hash, c.stored_est))
            .collect();
        let groups: Vec<String> = if with_groups {
            vec![
                format!("prefix:{}", fp.prefix_group),
                format!(
                    "ctype:{}",
                    fp.content_type.as_deref().unwrap_or("(unknown)")
                ),
            ]
        } else {
            Vec::new()
        };
        acc.observe_object(fp.logical_size, &groups, &chunks, |h, raw| {
            stored.get(h).copied().unwrap_or(raw)
        });
    }
    acc
}

fn chunk_view(fp: &ObjectFingerprint, view: View) -> &[crate::types::ChunkObs] {
    match view {
        View::Normalized => &fp.chunks,
        View::Opaque if fp.normalized => &fp.opaque_chunks,
        View::Opaque => &fp.chunks,
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SrfEstimate {
    pub point: f64,
    pub ci_low: f64,
    pub ci_high: f64,
    pub dedup_only: f64,
    /// How the CI was derived.
    pub ci_method: &'static str,
    #[serde(flatten)]
    pub stats: DedupStats,
}

/// Point estimate plus a 95% CI from an exact leave-one-out jackknife.
///
/// A naive bootstrap is biased here: resampling with replacement duplicates
/// objects, and a duplicated object dedups perfectly against itself, inflating
/// every resampled ratio above the point estimate. The jackknife has no such
/// artifact, and leave-one-out ratios are computable in O(total chunks) with
/// global chunk occurrence counts: removing object `i` only removes a chunk's
/// stored bytes if `i` held every occurrence of that chunk.
pub fn estimate_with_ci(fps: &[&ObjectFingerprint], view: View) -> SrfEstimate {
    let base = accumulate(fps, view, false);
    let stats = base.total().clone();
    let point = stats.srf();
    let dedup_only = stats.srf_dedup_only();

    let n = fps.len();
    let (ci_low, ci_high) = if n < 2 {
        (point, point)
    } else {
        // Global occurrence counts and first-wins stored sizes.
        let mut occurrences: HashMap<sparta_core::hash::ChunkHash, u32> = HashMap::new();
        let mut stored: HashMap<sparta_core::hash::ChunkHash, u64> = HashMap::new();
        for fp in fps {
            for c in chunk_view(fp, view) {
                *occurrences.entry(c.hash).or_insert(0) += 1;
                stored.entry(c.hash).or_insert(c.stored_est);
            }
        }
        let total_logical = stats.logical_bytes as f64;
        let total_stored = stats.unique_stored_bytes as f64;

        let thetas: Vec<f64> = fps
            .iter()
            .map(|fp| {
                // Count occurrences within this object (a chunk can repeat).
                let mut in_obj: HashMap<sparta_core::hash::ChunkHash, u32> = HashMap::new();
                for c in chunk_view(fp, view) {
                    *in_obj.entry(c.hash).or_insert(0) += 1;
                }
                let removed_stored: u64 = in_obj
                    .iter()
                    .filter(|(h, cnt)| occurrences[*h] == **cnt)
                    .map(|(h, _)| stored[h])
                    .sum();
                let l = total_logical - fp.logical_size as f64;
                let u = total_stored - removed_stored as f64;
                if u > 0.0 {
                    l / u
                } else {
                    point
                }
            })
            .collect();

        let mean = thetas.iter().sum::<f64>() / n as f64;
        let var = thetas.iter().map(|t| (t - mean) * (t - mean)).sum::<f64>() * (n as f64 - 1.0)
            / n as f64;
        let se = var.sqrt();
        (((point - 1.96 * se).max(0.0)), point + 1.96 * se)
    };

    SrfEstimate {
        point,
        ci_low,
        ci_high,
        dedup_only,
        ci_method: "jackknife-1.96se",
        stats,
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct GroupRow {
    pub key: String,
    pub object_count: u64,
    pub logical_bytes: u64,
    pub unique_stored_bytes: u64,
    pub srf: f64,
}

/// Grouped breakdown rows sorted by logical bytes, largest first.
pub fn group_rows(fps: &[&ObjectFingerprint], view: View, prefix: &str) -> Vec<GroupRow> {
    let acc = accumulate(fps, view, true);
    let mut rows: Vec<GroupRow> = acc
        .groups()
        .iter()
        .filter(|(k, _)| k.starts_with(prefix))
        .map(|(k, s)| GroupRow {
            key: k[prefix.len()..].to_string(),
            object_count: s.object_count,
            logical_bytes: s.logical_bytes,
            unique_stored_bytes: s.unique_stored_bytes,
            srf: s.srf(),
        })
        .collect();
    rows.sort_by_key(|r| std::cmp::Reverse(r.logical_bytes));
    rows
}

#[derive(Debug, Clone, Serialize)]
pub struct Cluster {
    pub member_count: usize,
    pub logical_bytes: u64,
    pub unique_stored_bytes: u64,
    pub srf: f64,
    /// Up to 5 example keys.
    pub sample_keys: Vec<String>,
}

/// Group sampled objects into redundancy clusters via MinHash LSH banding +
/// a Jaccard confirmation, then report the largest clusters by logical bytes.
pub fn clusters(fps: &[&ObjectFingerprint], jaccard_threshold: f64, top: usize) -> Vec<Cluster> {
    // Union-find over objects sharing at least one LSH band key, confirmed by
    // estimated Jaccard against the first member seen for that band.
    let mut parent: Vec<usize> = (0..fps.len()).collect();
    fn find(parent: &mut [usize], i: usize) -> usize {
        let mut i = i;
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    let mut by_band: HashMap<u64, Vec<usize>> = HashMap::new();
    let sketches: Vec<&MinHashSketch> = fps.iter().map(|f| &f.minhash).collect();
    for (i, fp) in fps.iter().enumerate() {
        for band in fp.minhash.band_keys() {
            by_band.entry(band).or_default().push(i);
        }
    }
    for members in by_band.values() {
        if members.len() < 2 {
            continue;
        }
        let first = members[0];
        for &j in &members[1..] {
            if sketches[first].jaccard(sketches[j]) >= jaccard_threshold {
                let (a, b) = (find(&mut parent, first), find(&mut parent, j));
                if a != b {
                    parent[a] = b;
                }
            }
        }
    }

    let mut groups: HashMap<usize, Vec<usize>> = HashMap::new();
    for i in 0..fps.len() {
        let root = find(&mut parent, i);
        groups.entry(root).or_default().push(i);
    }

    let mut out: Vec<Cluster> = groups
        .values()
        .filter(|members| members.len() >= 2)
        .map(|members| {
            let sel: Vec<&ObjectFingerprint> = members.iter().map(|&i| fps[i]).collect();
            let stats = accumulate(&sel, View::Normalized, false).total().clone();
            Cluster {
                member_count: members.len(),
                logical_bytes: stats.logical_bytes,
                unique_stored_bytes: stats.unique_stored_bytes,
                srf: stats.srf(),
                sample_keys: sel
                    .iter()
                    .take(5)
                    .map(|f| format!("{}/{}", f.bucket, f.key))
                    .collect(),
            }
        })
        .collect();
    out.sort_by_key(|c| std::cmp::Reverse(c.logical_bytes));
    out.truncate(top);
    out
}

/// Sample selection: a seeded uniform shuffle of population indices.
pub fn sample_indices(n: usize, target: usize, seed: u64) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..n).collect();
    let mut rng = StdRng::seed_from_u64(seed);
    // Partial Fisher–Yates: we only need the first `target` positions.
    let take = target.min(n);
    for i in 0..take {
        let j = rng.gen_range(i..n);
        idx.swap(i, j);
    }
    idx.truncate(take);
    idx
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ChunkObs;
    use sparta_core::hash::ChunkHash;
    use std::collections::HashSet;

    fn fp(key: &str, chunk_seeds: &[u64], excluded: bool) -> ObjectFingerprint {
        let chunks: Vec<ChunkObs> = chunk_seeds
            .iter()
            .map(|s| ChunkObs {
                hash: ChunkHash::of(&s.to_le_bytes()),
                raw_len: 1000,
                stored_est: 500,
            })
            .collect();
        let minhash = MinHashSketch::from_hashes(chunks.iter().map(|c| &c.hash));
        ObjectFingerprint {
            bucket: "b".into(),
            key: key.into(),
            logical_size: 1000 * chunk_seeds.len() as u64,
            full_size: 1000 * chunk_seeds.len() as u64,
            content_type: Some("application/x-tar".into()),
            prefix_group: "b/x/".into(),
            excluded,
            detected_compression: None,
            normalized: false,
            chunks,
            opaque_chunks: Vec::new(),
            minhash,
            truncated: false,
        }
    }

    #[test]
    fn five_identical_objects_srf_near_ten() {
        // 5 copies, each chunk stored at half size: SRF = 5x * 2x = 10.
        let objs: Vec<_> = (0..5)
            .map(|i| fp(&format!("k{i}"), &[1, 2, 3, 4], false))
            .collect();
        let refs: Vec<&ObjectFingerprint> = objs.iter().collect();
        let est = estimate_with_ci(&refs, View::Normalized);
        assert!((est.point - 10.0).abs() < 1e-9, "point {}", est.point);
        assert!(est.ci_low <= est.point && est.point <= est.ci_high);
    }

    #[test]
    fn jackknife_ci_brackets_point_for_mixed_population() {
        // 20 objects in a shared family plus 10 unique loners: heterogeneous
        // enough that leave-one-out ratios vary.
        let mut objs: Vec<_> = (0..20)
            .map(|i| fp(&format!("k{i}"), &[1, 2, 3], false))
            .collect();
        for i in 0..10u64 {
            objs.push(fp(
                &format!("solo{i}"),
                &[100 + i * 10, 101 + i * 10, 102 + i * 10],
                false,
            ));
        }
        let refs: Vec<&ObjectFingerprint> = objs.iter().collect();
        let est = estimate_with_ci(&refs, View::Normalized);
        assert!(
            est.ci_low <= est.point && est.point <= est.ci_high,
            "CI [{}, {}] must bracket point {}",
            est.ci_low,
            est.ci_high,
            est.point
        );
        assert!(est.ci_high > est.ci_low, "CI must have width: {est:?}");
        assert!(est.ci_low > 0.0);
    }

    #[test]
    fn clusters_find_template_families() {
        // Family A: 10 objects sharing 19/20 chunks; family B likewise;
        // 5 loners.
        let mut objs = Vec::new();
        for i in 0..10 {
            let mut seeds: Vec<u64> = (100..119).collect();
            seeds.push(1000 + i);
            objs.push(fp(&format!("a{i}"), &seeds, false));
        }
        for i in 0..10 {
            let mut seeds: Vec<u64> = (200..219).collect();
            seeds.push(2000 + i);
            objs.push(fp(&format!("b{i}"), &seeds, false));
        }
        for i in 0..5 {
            let seeds: Vec<u64> = (3000 + i * 50..3020 + i * 50).collect();
            objs.push(fp(&format!("loner{i}"), &seeds, false));
        }
        let refs: Vec<&ObjectFingerprint> = objs.iter().collect();
        let cl = clusters(&refs, 0.5, 10);
        assert!(cl.len() >= 2, "expected 2 clusters, got {}", cl.len());
        assert_eq!(cl[0].member_count, 10);
        assert_eq!(cl[1].member_count, 10);
        assert!(cl[0].srf > 5.0);
    }

    #[test]
    fn sampling_is_deterministic_and_uniform_ish() {
        let a = sample_indices(1000, 100, 42);
        let b = sample_indices(1000, 100, 42);
        assert_eq!(a, b);
        let c = sample_indices(1000, 100, 43);
        assert_ne!(a, c);
        let distinct: HashSet<_> = a.iter().collect();
        assert_eq!(distinct.len(), 100);
    }

    #[test]
    fn group_rows_split_by_prefix() {
        let mut objs = vec![fp("x1", &[1, 2], false)];
        let mut other = fp("y1", &[3, 4], false);
        other.prefix_group = "b/y/".into();
        objs.push(other);
        let refs: Vec<&ObjectFingerprint> = objs.iter().collect();
        let rows = group_rows(&refs, View::Normalized, "prefix:");
        assert_eq!(rows.len(), 2);
    }
}
