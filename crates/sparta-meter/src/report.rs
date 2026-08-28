//! Savings reporting: windowed NET savings, VSRR, and the invoice draft.
//!
//! All numbers derive from the meter log alone. Savings are NET by
//! construction: gross storage savings minus the request costs the pack layer
//! spends minus an engine compute estimate. Anything based on shadow-mode
//! projections is labeled `projected`.

use serde::Serialize;

use crate::pricing::RateCard;
use crate::record::{MeterRecord, Mode, RecordBody};

const MS_PER_MONTH: f64 = 730.0 * 3600.0 * 1000.0; // 730h billing month
const GIB: f64 = 1024.0 * 1024.0 * 1024.0;

#[derive(Debug, Clone, Serialize)]
pub struct SavingsReport {
    pub window_start_ms: u64,
    pub window_end_ms: u64,
    pub rate_source: String,

    /// Time-integrated storage in the window.
    pub logical_gib_months: f64,
    pub physical_gib_months: f64,
    /// True if any level record in the window was a projection (shadow).
    pub contains_projections: bool,

    pub gross_storage_savings_usd: f64,
    pub engine_request_cost_usd: f64,
    pub engine_compute_cost_usd: f64,
    pub net_savings_usd: f64,

    /// Net savings scaled to a 30-day rate, then annualized (x12.167).
    pub vsrr_30d_usd: f64,
    pub vsrr_annualized_usd: f64,

    pub ops: OpTotals,
    pub gc_sweeps: u64,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct OpTotals {
    pub client_requests: u64,
    pub client_logical_bytes: u64,
    pub client_physical_bytes: u64,
    pub engine_put_requests: u64,
    pub engine_get_requests: u64,
    pub engine_bytes: u64,
    pub engine_cpu_us: u64,
    pub shadow_ops: u64,
    pub active_ops: u64,
}

/// Compute the report over `[start_ms, end_ms]`.
pub fn savings_report(
    records: &[MeterRecord],
    start_ms: u64,
    end_ms: u64,
    card: &RateCard,
) -> SavingsReport {
    let mut totals = OpTotals::default();
    let mut gc_sweeps = 0u64;
    let mut contains_projections = false;

    // Step-hold integration of the level gauges across the window.
    let mut logical_byte_ms = 0f64;
    let mut physical_byte_ms = 0f64;
    let mut last_level: Option<(u64, u64, u64)> = None; // (ts, logical, physical)

    let mut integrate_to = |ts: u64, last: &Option<(u64, u64, u64)>| {
        if let Some((t0, l, p)) = last {
            let t0 = (*t0).max(start_ms);
            let t1 = ts.min(end_ms);
            if t1 > t0 {
                let dt = (t1 - t0) as f64;
                logical_byte_ms += *l as f64 * dt;
                physical_byte_ms += *p as f64 * dt;
            }
        }
    };

    for rec in records {
        match &rec.body {
            RecordBody::Level {
                logical_stored_bytes,
                physical_stored_bytes,
                projected,
            } => {
                // Levels before the window still establish the entry state.
                if rec.ts_ms > end_ms {
                    continue;
                }
                integrate_to(rec.ts_ms, &last_level);
                last_level = Some((rec.ts_ms, *logical_stored_bytes, *physical_stored_bytes));
                if *projected && rec.ts_ms >= start_ms {
                    contains_projections = true;
                }
            }
            RecordBody::Op {
                logical_bytes,
                physical_bytes,
                engine_put_requests,
                engine_get_requests,
                engine_bytes,
                engine_cpu_us,
                ..
            } if (start_ms..=end_ms).contains(&rec.ts_ms) => {
                totals.client_requests += 1;
                totals.client_logical_bytes += logical_bytes;
                totals.client_physical_bytes += physical_bytes;
                totals.engine_put_requests += engine_put_requests;
                totals.engine_get_requests += engine_get_requests;
                totals.engine_bytes += engine_bytes;
                totals.engine_cpu_us += engine_cpu_us;
                match rec.mode {
                    Mode::Shadow => totals.shadow_ops += 1,
                    Mode::Active => totals.active_ops += 1,
                }
            }
            RecordBody::GcSweep { dry_run: false, .. }
                if (start_ms..=end_ms).contains(&rec.ts_ms) =>
            {
                gc_sweeps += 1;
            }
            _ => {}
        }
    }
    // Hold the last level to the end of the window.
    integrate_to(end_ms, &last_level);

    let logical_gib_months = logical_byte_ms / GIB / MS_PER_MONTH;
    let physical_gib_months = physical_byte_ms / GIB / MS_PER_MONTH;

    let gross = card.storage_cost(logical_gib_months) - card.storage_cost(physical_gib_months);
    let request_cost = card.request_cost(totals.engine_put_requests, totals.engine_get_requests);
    let compute_cost = card.compute_cost(totals.engine_cpu_us);
    let net = gross - request_cost - compute_cost;

    let window_ms = (end_ms.saturating_sub(start_ms)).max(1) as f64;
    let vsrr_30d = net * (30.0 * 24.0 * 3600.0 * 1000.0) / window_ms;

    SavingsReport {
        window_start_ms: start_ms,
        window_end_ms: end_ms,
        rate_source: card.source.clone(),
        logical_gib_months,
        physical_gib_months,
        contains_projections,
        gross_storage_savings_usd: gross,
        engine_request_cost_usd: request_cost,
        engine_compute_cost_usd: compute_cost,
        net_savings_usd: net,
        vsrr_30d_usd: vsrr_30d,
        vsrr_annualized_usd: vsrr_30d * 12.166_666,
        ops: totals,
        gc_sweeps,
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct InvoiceDraft {
    pub period: String,
    pub generated_from: &'static str,
    pub report: SavingsReport,
    pub fee_fraction: f64,
    pub fee_usd: f64,
    pub note: String,
}

/// Monthly invoice draft: `month` is `YYYY-MM` (UTC).
pub fn invoice_draft(
    records: &[MeterRecord],
    month: &str,
    card: &RateCard,
    fee_fraction: f64,
) -> Result<InvoiceDraft, crate::MeterError> {
    let (start_ms, end_ms) = month_bounds(month)?;
    let report = savings_report(records, start_ms, end_ms, card);
    let fee = (report.net_savings_usd * fee_fraction).max(0.0);
    Ok(InvoiceDraft {
        period: month.to_string(),
        generated_from: "sparta-meter log (hash-chain verified)",
        fee_usd: fee,
        fee_fraction,
        note: if report.contains_projections {
            "Contains shadow-mode projections; verify with active-mode actuals before invoicing."
                .into()
        } else {
            "All figures derived from metered actuals.".into()
        },
        report,
    })
}

/// UTC month bounds without a chrono dependency (days-from-civil algorithm).
fn month_bounds(month: &str) -> Result<(u64, u64), crate::MeterError> {
    let parts: Vec<&str> = month.split('-').collect();
    let (Some(y), Some(m)) = (
        parts.first().and_then(|v| v.parse::<i64>().ok()),
        parts.get(1).and_then(|v| v.parse::<u32>().ok()),
    ) else {
        return Err(crate::MeterError::Invalid(format!(
            "month must be YYYY-MM, got {month:?}"
        )));
    };
    if !(1..=12).contains(&m) || parts.len() != 2 {
        return Err(crate::MeterError::Invalid(format!(
            "month must be YYYY-MM, got {month:?}"
        )));
    }
    let days = |y: i64, m: u32, d: u32| -> i64 {
        // Howard Hinnant's days_from_civil.
        let y = y - if m <= 2 { 1 } else { 0 };
        let era = if y >= 0 { y } else { y - 399 } / 400;
        let yoe = y - era * 400;
        let mp = (m as i64 + 9) % 12;
        let doy = (153 * mp + 2) / 5 + d as i64 - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146_097 + doe - 719_468
    };
    let start = days(y, m, 1) * 86_400_000;
    let (ny, nm) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
    let end = days(ny, nm, 1) * 86_400_000;
    Ok((start.max(0) as u64, end.max(0) as u64))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::GENESIS_PREV;

    fn seal_chain(mut records: Vec<MeterRecord>) -> Vec<MeterRecord> {
        let mut prev = GENESIS_PREV.to_string();
        for r in records.iter_mut() {
            *r = r.clone().seal(&prev);
            prev = r.hash.clone();
        }
        records
    }

    fn rec(seq: u64, ts_ms: u64, mode: Mode, body: RecordBody) -> MeterRecord {
        MeterRecord {
            seq,
            ts_ms,
            mode,
            body,
            prev: String::new(),
            hash: String::new(),
        }
    }

    const DAY: u64 = 86_400_000;

    /// 30 days of steady state: 10 TiB logical, 1 TiB physical.
    fn steady_state_log() -> Vec<MeterRecord> {
        let tib = 1u64 << 40;
        let mut v = Vec::new();
        for d in 0..=30u64 {
            v.push(rec(
                d,
                d * DAY,
                Mode::Active,
                RecordBody::Level {
                    logical_stored_bytes: 10 * tib,
                    physical_stored_bytes: tib,
                    projected: false,
                },
            ));
        }
        seal_chain(v)
    }

    #[test]
    fn steady_state_savings_math() {
        let records = steady_state_log();
        let card = RateCard::aws_us_east_1();
        let r = savings_report(&records, 0, 30 * DAY, &card);
        // 30 days ≈ 720h of a 730h month.
        let month_frac = 720.0 / 730.0;
        let expect_logical = 10.0 * 1024.0 * month_frac;
        assert!(
            (r.logical_gib_months - expect_logical).abs() < 1.0,
            "logical {} vs {}",
            r.logical_gib_months,
            expect_logical
        );
        assert!(r.gross_storage_savings_usd > 0.0);
        // 9 TiB * ~0.72 month * 0.023 ≈ $152.6
        assert!(
            (r.gross_storage_savings_usd - 9.0 * 1024.0 * month_frac * 0.023).abs() < 1.0,
            "gross {}",
            r.gross_storage_savings_usd
        );
        assert_eq!(r.net_savings_usd, r.gross_storage_savings_usd); // no overheads
        assert!(!r.contains_projections);
        assert!(r.vsrr_annualized_usd > r.net_savings_usd * 12.0);
    }

    #[test]
    fn net_subtracts_engine_costs() {
        let mut records = steady_state_log();
        let n = records.len() as u64;
        records.push(rec(
            n,
            15 * DAY,
            Mode::Active,
            RecordBody::Op {
                op: "put_object".into(),
                bucket: "b".into(),
                logical_bytes: 1 << 30,
                physical_bytes: 1 << 27,
                engine_put_requests: 100_000,
                engine_get_requests: 1_000_000,
                engine_bytes: 1 << 27,
                engine_cpu_us: 3_600_000_000, // 1 vCPU-hour
            },
        ));
        // Re-seal the chain after appending.
        let mut prev = crate::record::GENESIS_PREV.to_string();
        for r in records.iter_mut() {
            let mut plain = r.clone();
            plain.prev = String::new();
            plain.hash = String::new();
            *r = plain.seal(&prev);
            prev = r.hash.clone();
        }

        let card = RateCard::aws_us_east_1();
        let r = savings_report(&records, 0, 30 * DAY, &card);
        let expect_requests = 100.0 * 0.005 + 1000.0 * 0.0004;
        assert!((r.engine_request_cost_usd - expect_requests).abs() < 1e-6);
        assert!((r.engine_compute_cost_usd - 0.0425).abs() < 1e-6);
        assert!(
            (r.gross_storage_savings_usd - r.net_savings_usd - expect_requests - 0.0425).abs()
                < 1e-6
        );
    }

    #[test]
    fn projections_are_labeled() {
        let records = seal_chain(vec![rec(
            0,
            DAY,
            Mode::Shadow,
            RecordBody::Level {
                logical_stored_bytes: 1 << 40,
                physical_stored_bytes: 1 << 37,
                projected: true,
            },
        )]);
        let card = RateCard::aws_us_east_1();
        let r = savings_report(&records, 0, 2 * DAY, &card);
        assert!(r.contains_projections);
    }

    #[test]
    fn invoice_draft_charges_fee_on_net() {
        let records = steady_state_log();
        let card = RateCard::aws_us_east_1();
        // Timestamps are relative (day 0..30) — use a window-style month via
        // savings_report; invoice_draft needs real months, so shift records
        // into 2026-08.
        let aug_start = super::month_bounds("2026-08").unwrap().0;
        let shifted: Vec<MeterRecord> = {
            let mut v: Vec<MeterRecord> = records
                .iter()
                .map(|r| {
                    let mut x = r.clone();
                    x.ts_ms += aug_start;
                    x.prev = String::new();
                    x.hash = String::new();
                    x
                })
                .collect();
            let mut prev = crate::record::GENESIS_PREV.to_string();
            for r in v.iter_mut() {
                *r = r.clone().seal(&prev);
                prev = r.hash.clone();
            }
            v
        };
        let inv = invoice_draft(&shifted, "2026-08", &card, 0.2).unwrap();
        assert!(inv.fee_usd > 0.0);
        assert!((inv.fee_usd - inv.report.net_savings_usd * 0.2).abs() < 1e-9);
    }

    #[test]
    fn month_bounds_are_utc_calendar_months() {
        let (s, e) = super::month_bounds("2026-08").unwrap();
        assert_eq!((e - s) / DAY, 31);
        let (s2, _) = super::month_bounds("2026-09").unwrap();
        assert_eq!(s2, e);
        assert!(super::month_bounds("2026-13").is_err());
        assert!(super::month_bounds("garbage").is_err());
    }
}
