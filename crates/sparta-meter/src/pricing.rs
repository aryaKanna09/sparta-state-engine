//! Pricing: a built-in AWS S3 rate card plus a loader that derives the
//! customer's real effective rates from an AWS Cost & Usage Report export.

use std::io::Read;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::MeterError;

/// Effective rates used to price savings. All storage rates are $/GiB-month,
/// request rates $/1000 requests, transfer $/GiB.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RateCard {
    pub source: String,
    /// Tiered standard storage: (tier ceiling in GiB-months, rate). The last
    /// tier's ceiling is u64::MAX.
    pub storage_tiers: Vec<(u64, f64)>,
    /// PUT/COPY/POST/LIST class.
    pub put_per_1k: f64,
    /// GET/SELECT class.
    pub get_per_1k: f64,
    /// Data transfer out to internet.
    pub egress_per_gib: f64,
    /// Engine compute pricing for the NET calculation, $/vCPU-hour.
    pub compute_per_vcpu_hour: f64,
}

impl RateCard {
    /// AWS S3 Standard, us-east-1, public on-demand list prices.
    pub fn aws_us_east_1() -> Self {
        Self {
            source: "builtin:aws-s3-standard-us-east-1".into(),
            storage_tiers: vec![
                (50 * 1024, 0.023),  // first 50 TB
                (500 * 1024, 0.022), // next 450 TB
                (u64::MAX, 0.021),   // over 500 TB
            ],
            put_per_1k: 0.005,
            get_per_1k: 0.0004,
            egress_per_gib: 0.09,
            compute_per_vcpu_hour: 0.0425, // c6i on-demand per-vCPU
        }
    }

    /// Cost of storing `gib_months` at the tiered rate.
    pub fn storage_cost(&self, gib_months: f64) -> f64 {
        let mut remaining = gib_months.max(0.0);
        let mut floor = 0f64;
        let mut cost = 0f64;
        for (ceil, rate) in &self.storage_tiers {
            let ceil = *ceil as f64;
            let in_tier = (remaining).min(ceil - floor);
            if in_tier <= 0.0 {
                break;
            }
            cost += in_tier * rate;
            remaining -= in_tier;
            floor = ceil;
            if remaining <= 0.0 {
                break;
            }
        }
        cost
    }

    pub fn request_cost(&self, put_class: u64, get_class: u64) -> f64 {
        put_class as f64 / 1000.0 * self.put_per_1k + get_class as f64 / 1000.0 * self.get_per_1k
    }

    pub fn compute_cost(&self, cpu_us: u64) -> f64 {
        cpu_us as f64 / 1e6 / 3600.0 * self.compute_per_vcpu_hour
    }
}

/// Derive effective rates from an AWS CUR export (CSV, optionally gzip).
///
/// We aggregate AmazonS3 line items and compute effective $/unit for the
/// usage classes we price: TimedStorage (GB-month), Requests-Tier1 (PUT
/// class), Requests-Tier2 (GET class), DataTransfer-Out-Bytes. Classes absent
/// from the report keep the built-in default. This intentionally reads only
/// stable CUR columns: lineItem/ProductCode, lineItem/UsageType,
/// lineItem/UsageAmount, lineItem/UnblendedCost.
pub fn ratecard_from_cur(path: &Path) -> Result<RateCard, MeterError> {
    let raw = std::fs::read(path)?;
    let text: Vec<u8> = if raw.starts_with(&[0x1f, 0x8b]) {
        let mut out = Vec::new();
        flate2::read::MultiGzDecoder::new(&raw[..])
            .read_to_end(&mut out)
            .map_err(MeterError::Io)?;
        out
    } else {
        raw
    };
    let mut rdr = csv::Reader::from_reader(&text[..]);
    let headers = rdr.headers().map_err(cur_err)?.clone();
    let col = |name: &str| headers.iter().position(|h| h == name);
    let (Some(c_product), Some(c_usage_type), Some(c_amount), Some(c_cost)) = (
        col("lineItem/ProductCode"),
        col("lineItem/UsageType"),
        col("lineItem/UsageAmount"),
        col("lineItem/UnblendedCost"),
    ) else {
        return Err(MeterError::Invalid(
            "CUR file missing lineItem columns (ProductCode/UsageType/UsageAmount/UnblendedCost)"
                .into(),
        ));
    };

    #[derive(Default)]
    struct Agg {
        amount: f64,
        cost: f64,
    }
    let (mut storage, mut tier1, mut tier2, mut egress) = (
        Agg::default(),
        Agg::default(),
        Agg::default(),
        Agg::default(),
    );

    for rec in rdr.records() {
        let rec = rec.map_err(cur_err)?;
        if rec.get(c_product) != Some("AmazonS3") {
            continue;
        }
        let usage_type = rec.get(c_usage_type).unwrap_or("");
        let amount: f64 = rec
            .get(c_amount)
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.0);
        let cost: f64 = rec.get(c_cost).and_then(|v| v.parse().ok()).unwrap_or(0.0);
        let bucket = if usage_type.contains("TimedStorage-ByteHrs") {
            &mut storage
        } else if usage_type.contains("Requests-Tier1") {
            &mut tier1
        } else if usage_type.contains("Requests-Tier2") {
            &mut tier2
        } else if usage_type.contains("DataTransfer-Out-Bytes") {
            &mut egress
        } else {
            continue;
        };
        bucket.amount += amount;
        bucket.cost += cost;
    }

    let mut card = RateCard::aws_us_east_1();
    card.source = format!("cur:{}", path.display());
    if storage.amount > 0.0 {
        // CUR TimedStorage usage amount is GB-month.
        let rate = storage.cost / storage.amount;
        card.storage_tiers = vec![(u64::MAX, rate)];
    }
    if tier1.amount > 0.0 {
        card.put_per_1k = tier1.cost / tier1.amount * 1000.0;
    }
    if tier2.amount > 0.0 {
        card.get_per_1k = tier2.cost / tier2.amount * 1000.0;
    }
    if egress.amount > 0.0 {
        // Usage amount is GB.
        card.egress_per_gib = egress.cost / egress.amount;
    }
    Ok(card)
}

fn cur_err(e: csv::Error) -> MeterError {
    MeterError::Invalid(format!("CUR parse: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn tiered_storage_cost() {
        let card = RateCard::aws_us_east_1();
        // 100 GiB fully in tier 1.
        assert!((card.storage_cost(100.0) - 2.3).abs() < 1e-9);
        // 60 TiB spans tiers 1 and 2.
        let cost = card.storage_cost(60.0 * 1024.0);
        let expect = 50.0 * 1024.0 * 0.023 + 10.0 * 1024.0 * 0.022;
        assert!((cost - expect).abs() < 1e-6);
    }

    #[test]
    fn cur_overrides_rates() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cur.csv");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(
            f,
            "lineItem/ProductCode,lineItem/UsageType,lineItem/UsageAmount,lineItem/UnblendedCost"
        )
        .unwrap();
        // Effective storage rate 0.02 $/GB-month, PUT 0.004/1k, GET 0.0003/1k,
        // egress 0.05/GB. Includes a non-S3 row that must be ignored.
        writeln!(f, "AmazonS3,USE1-TimedStorage-ByteHrs,1000,20.0").unwrap();
        writeln!(f, "AmazonS3,USE1-Requests-Tier1,500000,2.0").unwrap();
        writeln!(f, "AmazonS3,USE1-Requests-Tier2,1000000,0.3").unwrap();
        writeln!(f, "AmazonS3,USE1-DataTransfer-Out-Bytes,200,10.0").unwrap();
        writeln!(f, "AmazonEC2,BoxUsage:m6i.large,100,9.6").unwrap();
        drop(f);

        let card = ratecard_from_cur(&path).unwrap();
        assert_eq!(card.storage_tiers, vec![(u64::MAX, 0.02)]);
        assert!((card.put_per_1k - 0.004).abs() < 1e-9);
        assert!((card.get_per_1k - 0.0003).abs() < 1e-9);
        assert!((card.egress_per_gib - 0.05).abs() < 1e-9);
        assert!(card.source.starts_with("cur:"));
    }

    #[test]
    fn cur_missing_columns_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.csv");
        std::fs::write(&path, "a,b,c\n1,2,3\n").unwrap();
        assert!(ratecard_from_cur(&path).is_err());
    }
}
