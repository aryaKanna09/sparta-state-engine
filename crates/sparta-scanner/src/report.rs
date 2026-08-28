//! Scanner report: a JSON document plus a self-contained single-file HTML
//! rendering suitable for sending to a customer.

use serde::Serialize;

use crate::estimate::{Cluster, GroupRow, SrfEstimate};
use crate::types::PopulationStats;

#[derive(Debug, Serialize)]
pub struct Report {
    pub tool: &'static str,
    pub version: &'static str,
    pub generated_unix_ms: u64,
    pub spec: SpecParams,
    pub buckets: Vec<String>,
    pub seed: u64,
    pub sample_rate: f64,
    pub byte_budget: u64,
    pub bytes_scanned: u64,
    pub population: PopulationStats,
    pub sample: SampleStats,
    /// SRF-S with compression normalization (the spec pipeline).
    pub srf_s: SrfEstimate,
    /// SRF-S as if normalization were disabled.
    pub srf_s_no_normalization: SrfEstimate,
    /// srf_s.point / srf_s_no_normalization.point.
    pub normalization_delta_factor: f64,
    /// True when normalization moved the estimate by more than 3x either way;
    /// the report is then marked as requiring reverification.
    pub requires_reverification: bool,
    /// SRF-S over the sample with excluded objects removed.
    pub addressable: AddressableStats,
    pub by_prefix: Vec<GroupRow>,
    pub by_content_type: Vec<GroupRow>,
    pub top_clusters: Vec<Cluster>,
    pub caveats: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct SpecParams {
    pub cdc_min: u32,
    pub cdc_avg: u32,
    pub cdc_max: u32,
    pub hash: &'static str,
    pub pack_zstd_level: i32,
    pub normalization_default: bool,
}

impl Default for SpecParams {
    fn default() -> Self {
        Self {
            cdc_min: sparta_core::spec::CDC_MIN_SIZE,
            cdc_avg: sparta_core::spec::CDC_AVG_SIZE,
            cdc_max: sparta_core::spec::CDC_MAX_SIZE,
            hash: "blake3-256",
            pack_zstd_level: sparta_core::spec::PACK_ZSTD_LEVEL,
            normalization_default: true,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct SampleStats {
    pub object_count: u64,
    pub logical_bytes: u64,
    /// Bytes of sampled objects beyond the per-object cap (not fetched).
    pub unscanned_tail_bytes: u64,
    /// Sampled objects recognized as compressed by magic bytes.
    pub compressed_objects_detected: u64,
    /// Sampled objects whose normalization verified byte-exactly.
    pub normalized_objects: u64,
    pub truncated_objects: u64,
    pub fetch_errors: u64,
    pub byte_coverage_fraction: f64,
    pub object_coverage_fraction: f64,
    pub budget_exhausted: bool,
}

#[derive(Debug, Serialize)]
pub struct AddressableStats {
    pub excluded_sampled_objects: u64,
    pub excluded_sampled_bytes: u64,
    pub srf: SrfEstimate,
}

pub fn fmt_bytes(b: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"];
    let mut v = b as f64;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 {
        format!("{b} B")
    } else {
        format!("{v:.2} {}", UNITS[u])
    }
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

impl Report {
    pub fn to_json(&self) -> serde_json::Result<String> {
        serde_json::to_string_pretty(self)
    }

    /// Render the single-file HTML report. No external assets, no scripts.
    pub fn to_html(&self) -> String {
        let mut out = String::with_capacity(64 * 1024);
        let srf = &self.srf_s;
        let warn_block = if self.requires_reverification {
            format!(
                r#"<div class="banner warn"><strong>Requires reverification.</strong>
Compression normalization changed the redundancy estimate by {:.1}&times;
(with: {:.2}, without: {:.2}). Estimates this sensitive to normalization
should be confirmed with a larger sample or a Shadow B deployment before
being used in sizing decisions.</div>"#,
                if self.normalization_delta_factor >= 1.0 {
                    self.normalization_delta_factor
                } else {
                    1.0 / self.normalization_delta_factor
                },
                self.srf_s.point,
                self.srf_s_no_normalization.point
            )
        } else {
            String::new()
        };

        out.push_str(&format!(
            r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Sparta SRF-S Report</title>
<style>
:root {{ color-scheme: light; }}
body {{ font: 15px/1.5 -apple-system, BlinkMacSystemFont, "Segoe UI", Helvetica, Arial, sans-serif;
        margin: 0; background: #f6f7f9; color: #1a1d21; }}
.page {{ max-width: 960px; margin: 0 auto; padding: 32px 24px 64px; }}
h1 {{ font-size: 26px; margin: 0 0 4px; }}
h2 {{ font-size: 18px; margin: 36px 0 10px; }}
.sub {{ color: #5b6470; margin: 0 0 24px; }}
.banner {{ padding: 14px 18px; border-radius: 8px; margin: 18px 0; }}
.warn {{ background: #fff3e0; border: 1px solid #f0b429; }}
.grid {{ display: grid; grid-template-columns: repeat(auto-fit, minmax(200px, 1fr)); gap: 14px; margin: 18px 0; }}
.card {{ background: #fff; border: 1px solid #e3e6ea; border-radius: 10px; padding: 16px 18px; }}
.card .big {{ font-size: 30px; font-weight: 700; }}
.card .label {{ color: #5b6470; font-size: 13px; text-transform: uppercase; letter-spacing: .04em; }}
.card .detail {{ color: #5b6470; font-size: 13px; margin-top: 4px; }}
table {{ width: 100%; border-collapse: collapse; background: #fff; border: 1px solid #e3e6ea; border-radius: 10px; overflow: hidden; }}
th, td {{ text-align: left; padding: 9px 12px; border-bottom: 1px solid #eef0f3; font-size: 14px; }}
th {{ background: #fafbfc; color: #5b6470; font-weight: 600; }}
tr:last-child td {{ border-bottom: none; }}
td.num, th.num {{ text-align: right; font-variant-numeric: tabular-nums; }}
.bar {{ background: #e8ecf1; border-radius: 4px; height: 8px; min-width: 60px; }}
.bar > div {{ background: #2f6fed; height: 8px; border-radius: 4px; }}
.mono {{ font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size: 13px; }}
.foot {{ color: #8a93a0; font-size: 12px; margin-top: 40px; }}
</style></head><body><div class="page">
<h1>Sparta State Engine &mdash; SRF-S Scan Report</h1>
<p class="sub">Bucket(s): {buckets} &middot; sample rate {rate:.2}% &middot; seed {seed}</p>
{warn}
<div class="grid">
  <div class="card"><div class="label">SRF-S (sampled)</div>
    <div class="big">{srf:.2}&times;</div>
    <div class="detail">95% CI {lo:.2}&ndash;{hi:.2} &middot; dedup-only {dd:.2}&times;</div></div>
  <div class="card"><div class="label">Addressable SRF-S</div>
    <div class="big">{asrf:.2}&times;</div>
    <div class="detail">{aexcl} sampled objects excluded by rules</div></div>
  <div class="card"><div class="label">Population</div>
    <div class="big">{pop_n}</div>
    <div class="detail">{pop_b} across {nbuckets} bucket(s){inv}</div></div>
  <div class="card"><div class="label">Sample</div>
    <div class="big">{s_n}</div>
    <div class="detail">{s_b} fingerprinted ({cov:.1}% of bytes){budget}</div></div>
</div>
"#,
            buckets = esc(&self.buckets.join(", ")),
            rate = self.sample_rate * 100.0,
            seed = self.seed,
            warn = warn_block,
            srf = srf.point,
            lo = srf.ci_low,
            hi = srf.ci_high,
            dd = srf.dedup_only,
            asrf = self.addressable.srf.point,
            aexcl = self.addressable.excluded_sampled_objects,
            pop_n = self.population.object_count,
            pop_b = fmt_bytes(self.population.total_bytes),
            nbuckets = self.buckets.len(),
            inv = if self.population.from_inventory {
                " (from S3 Inventory)"
            } else {
                ""
            },
            s_n = self.sample.object_count,
            s_b = fmt_bytes(self.sample.logical_bytes),
            cov = self.sample.byte_coverage_fraction * 100.0,
            budget = if self.sample.budget_exhausted {
                " &middot; byte budget exhausted"
            } else {
                ""
            },
        ));

        out.push_str(&format!(
            r#"<h2>Normalization impact</h2>
<table><tr><th>Pipeline</th><th class="num">SRF-S</th><th class="num">Unique stored</th></tr>
<tr><td>With compression normalization (spec default)</td><td class="num">{:.2}&times;</td><td class="num">{}</td></tr>
<tr><td>Without normalization</td><td class="num">{:.2}&times;</td><td class="num">{}</td></tr>
</table>"#,
            self.srf_s.point,
            fmt_bytes(self.srf_s.stats.unique_stored_bytes),
            self.srf_s_no_normalization.point,
            fmt_bytes(self.srf_s_no_normalization.stats.unique_stored_bytes),
        ));

        let table = |title: &str, rows: &[GroupRow]| -> String {
            if rows.is_empty() {
                return String::new();
            }
            let max = rows
                .iter()
                .map(|r| r.logical_bytes)
                .max()
                .unwrap_or(1)
                .max(1);
            let mut t = format!(
                r#"<h2>{title}</h2>
<table><tr><th>Group</th><th class="num">Objects</th><th class="num">Logical</th>
<th class="num">Stored (est)</th><th class="num">SRF</th><th>Share</th></tr>"#
            );
            for r in rows.iter().take(20) {
                t.push_str(&format!(
                    r#"<tr><td class="mono">{}</td><td class="num">{}</td><td class="num">{}</td>
<td class="num">{}</td><td class="num">{:.2}&times;</td>
<td><div class="bar"><div style="width:{}%"></div></div></td></tr>"#,
                    esc(&r.key),
                    r.object_count,
                    fmt_bytes(r.logical_bytes),
                    fmt_bytes(r.unique_stored_bytes),
                    r.srf,
                    (r.logical_bytes as f64 / max as f64 * 100.0).round(),
                ));
            }
            t.push_str("</table>");
            t
        };
        out.push_str(&table("Breakdown by prefix (sampled)", &self.by_prefix));
        out.push_str(&table(
            "Breakdown by content type (sampled)",
            &self.by_content_type,
        ));

        if !self.top_clusters.is_empty() {
            out.push_str(
                r#"<h2>Top redundancy clusters</h2>
<table><tr><th>Members</th><th class="num">Logical</th><th class="num">Stored (est)</th>
<th class="num">SRF</th><th>Example keys</th></tr>"#,
            );
            for c in &self.top_clusters {
                out.push_str(&format!(
                    r#"<tr><td class="num">{}</td><td class="num">{}</td><td class="num">{}</td>
<td class="num">{:.2}&times;</td><td class="mono">{}</td></tr>"#,
                    c.member_count,
                    fmt_bytes(c.logical_bytes),
                    fmt_bytes(c.unique_stored_bytes),
                    c.srf,
                    esc(&c.sample_keys.join("<br>")).replace("&lt;br&gt;", "<br>"),
                ));
            }
            out.push_str("</table>");
        }

        if !self.caveats.is_empty() {
            out.push_str("<h2>Method notes and caveats</h2><ul>");
            for c in &self.caveats {
                out.push_str(&format!("<li>{}</li>", esc(c)));
            }
            out.push_str("</ul>");
        }

        out.push_str(&format!(
            r#"<p class="foot">Generated by sparta-scanner v{} &middot; SRF spec: FastCDC {}/{}/{} KiB&middot;BLAKE3-256&middot;zstd&nbsp;L{} &middot; read-only scan; no data left the environment.</p>
</div></body></html>"#,
            self.version,
            self.spec.cdc_min / 1024,
            self.spec.cdc_avg / 1024,
            self.spec.cdc_max / 1024,
            self.spec.pack_zstd_level,
        ));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_formatting() {
        assert_eq!(fmt_bytes(512), "512 B");
        assert_eq!(fmt_bytes(1536), "1.50 KiB");
        assert_eq!(fmt_bytes(48 * 1024 * 1024), "48.00 MiB");
    }
}
