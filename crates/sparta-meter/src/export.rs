//! Meter log exports: flattened CSV and Parquet with a fixed column set.

use std::fs::File;
use std::path::Path;
use std::sync::Arc;

use parquet::data_type::{BoolType, ByteArray, ByteArrayType, Int64Type};
use parquet::file::properties::WriterProperties;
use parquet::file::writer::SerializedFileWriter;
use parquet::schema::parser::parse_message_type;

use crate::record::{MeterRecord, Mode, RecordBody};
use crate::MeterError;

/// One flattened export row; None encodes NULL for non-applicable columns.
struct Row {
    seq: i64,
    ts_ms: i64,
    mode: &'static str,
    kind: &'static str,
    op: Option<String>,
    bucket: Option<String>,
    logical_bytes: Option<i64>,
    physical_bytes: Option<i64>,
    engine_put_requests: Option<i64>,
    engine_get_requests: Option<i64>,
    engine_bytes: Option<i64>,
    engine_cpu_us: Option<i64>,
    logical_stored_bytes: Option<i64>,
    physical_stored_bytes: Option<i64>,
    projected: Option<bool>,
    packs_deleted: Option<i64>,
    bytes_freed: Option<i64>,
    chunks_freed: Option<i64>,
    dry_run: Option<bool>,
    note: Option<String>,
    hash: String,
}

fn flatten(rec: &MeterRecord) -> Row {
    let mut row = Row {
        seq: rec.seq as i64,
        ts_ms: rec.ts_ms as i64,
        mode: match rec.mode {
            Mode::Shadow => "shadow",
            Mode::Active => "active",
        },
        kind: "",
        op: None,
        bucket: None,
        logical_bytes: None,
        physical_bytes: None,
        engine_put_requests: None,
        engine_get_requests: None,
        engine_bytes: None,
        engine_cpu_us: None,
        logical_stored_bytes: None,
        physical_stored_bytes: None,
        projected: None,
        packs_deleted: None,
        bytes_freed: None,
        chunks_freed: None,
        dry_run: None,
        note: None,
        hash: rec.hash.clone(),
    };
    match &rec.body {
        RecordBody::Op {
            op,
            bucket,
            logical_bytes,
            physical_bytes,
            engine_put_requests,
            engine_get_requests,
            engine_bytes,
            engine_cpu_us,
        } => {
            row.kind = "op";
            row.op = Some(op.clone());
            row.bucket = Some(bucket.clone());
            row.logical_bytes = Some(*logical_bytes as i64);
            row.physical_bytes = Some(*physical_bytes as i64);
            row.engine_put_requests = Some(*engine_put_requests as i64);
            row.engine_get_requests = Some(*engine_get_requests as i64);
            row.engine_bytes = Some(*engine_bytes as i64);
            row.engine_cpu_us = Some(*engine_cpu_us as i64);
        }
        RecordBody::Level {
            logical_stored_bytes,
            physical_stored_bytes,
            projected,
        } => {
            row.kind = "level";
            row.logical_stored_bytes = Some(*logical_stored_bytes as i64);
            row.physical_stored_bytes = Some(*physical_stored_bytes as i64);
            row.projected = Some(*projected);
        }
        RecordBody::GcSweep {
            packs_deleted,
            bytes_freed,
            chunks_freed,
            dry_run,
        } => {
            row.kind = "gc_sweep";
            row.packs_deleted = Some(*packs_deleted as i64);
            row.bytes_freed = Some(*bytes_freed as i64);
            row.chunks_freed = Some(*chunks_freed as i64);
            row.dry_run = Some(*dry_run);
        }
        RecordBody::Note { text } => {
            row.kind = "note";
            row.note = Some(text.clone());
        }
    }
    row
}

const COLUMNS: [&str; 21] = [
    "seq",
    "ts_ms",
    "mode",
    "kind",
    "op",
    "bucket",
    "logical_bytes",
    "physical_bytes",
    "engine_put_requests",
    "engine_get_requests",
    "engine_bytes",
    "engine_cpu_us",
    "logical_stored_bytes",
    "physical_stored_bytes",
    "projected",
    "packs_deleted",
    "bytes_freed",
    "chunks_freed",
    "dry_run",
    "note",
    "hash",
];

pub fn export_csv(records: &[MeterRecord], out: &Path) -> Result<(), MeterError> {
    let mut w = csv::Writer::from_path(out).map_err(csv_err)?;
    w.write_record(COLUMNS).map_err(csv_err)?;
    let opt_i = |v: Option<i64>| v.map(|x| x.to_string()).unwrap_or_default();
    let opt_b = |v: Option<bool>| v.map(|x| x.to_string()).unwrap_or_default();
    for rec in records {
        let r = flatten(rec);
        w.write_record([
            r.seq.to_string(),
            r.ts_ms.to_string(),
            r.mode.to_string(),
            r.kind.to_string(),
            r.op.unwrap_or_default(),
            r.bucket.unwrap_or_default(),
            opt_i(r.logical_bytes),
            opt_i(r.physical_bytes),
            opt_i(r.engine_put_requests),
            opt_i(r.engine_get_requests),
            opt_i(r.engine_bytes),
            opt_i(r.engine_cpu_us),
            opt_i(r.logical_stored_bytes),
            opt_i(r.physical_stored_bytes),
            opt_b(r.projected),
            opt_i(r.packs_deleted),
            opt_i(r.bytes_freed),
            opt_i(r.chunks_freed),
            opt_b(r.dry_run),
            r.note.unwrap_or_default(),
            r.hash,
        ])
        .map_err(csv_err)?;
    }
    w.flush()?;
    Ok(())
}

fn csv_err(e: csv::Error) -> MeterError {
    MeterError::Invalid(format!("csv export: {e}"))
}

fn pq_err(e: parquet::errors::ParquetError) -> MeterError {
    MeterError::Invalid(format!("parquet export: {e}"))
}

pub fn export_parquet(records: &[MeterRecord], out: &Path) -> Result<(), MeterError> {
    let schema = parse_message_type(
        "message meter {
            required int64 seq;
            required int64 ts_ms;
            required binary mode (UTF8);
            required binary kind (UTF8);
            optional binary op (UTF8);
            optional binary bucket (UTF8);
            optional int64 logical_bytes;
            optional int64 physical_bytes;
            optional int64 engine_put_requests;
            optional int64 engine_get_requests;
            optional int64 engine_bytes;
            optional int64 engine_cpu_us;
            optional int64 logical_stored_bytes;
            optional int64 physical_stored_bytes;
            optional boolean projected;
            optional int64 packs_deleted;
            optional int64 bytes_freed;
            optional int64 chunks_freed;
            optional boolean dry_run;
            optional binary note (UTF8);
            required binary hash (UTF8);
        }",
    )
    .map_err(pq_err)?;
    let file = File::create(out)?;
    let mut writer = SerializedFileWriter::new(
        file,
        Arc::new(schema),
        Arc::new(WriterProperties::builder().build()),
    )
    .map_err(pq_err)?;

    let rows: Vec<Row> = records.iter().map(flatten).collect();

    // Column write helpers. Optionals use def level 1 = present, 0 = null.
    fn opt_i64(rows: &[Row], f: impl Fn(&Row) -> Option<i64>) -> (Vec<i64>, Vec<i16>) {
        let mut vals = Vec::new();
        let mut defs = Vec::with_capacity(rows.len());
        for r in rows {
            match f(r) {
                Some(v) => {
                    vals.push(v);
                    defs.push(1);
                }
                None => defs.push(0),
            }
        }
        (vals, defs)
    }
    fn opt_bool(rows: &[Row], f: impl Fn(&Row) -> Option<bool>) -> (Vec<bool>, Vec<i16>) {
        let mut vals = Vec::new();
        let mut defs = Vec::with_capacity(rows.len());
        for r in rows {
            match f(r) {
                Some(v) => {
                    vals.push(v);
                    defs.push(1);
                }
                None => defs.push(0),
            }
        }
        (vals, defs)
    }
    fn opt_str(rows: &[Row], f: impl Fn(&Row) -> Option<&str>) -> (Vec<ByteArray>, Vec<i16>) {
        let mut vals = Vec::new();
        let mut defs = Vec::with_capacity(rows.len());
        for r in rows {
            match f(r) {
                Some(v) => {
                    vals.push(ByteArray::from(v.as_bytes().to_vec()));
                    defs.push(1);
                }
                None => defs.push(0),
            }
        }
        (vals, defs)
    }

    let mut rg = writer.next_row_group().map_err(pq_err)?;
    let mut col_idx = 0usize;
    while let Some(mut col) = rg.next_column().map_err(pq_err)? {
        {
            let writer = &mut col;
            match COLUMNS[col_idx] {
                "seq" => {
                    let vals: Vec<i64> = rows.iter().map(|r| r.seq).collect();
                    writer
                        .typed::<Int64Type>()
                        .write_batch(&vals, None, None)
                        .map_err(pq_err)?;
                }
                "ts_ms" => {
                    let vals: Vec<i64> = rows.iter().map(|r| r.ts_ms).collect();
                    writer
                        .typed::<Int64Type>()
                        .write_batch(&vals, None, None)
                        .map_err(pq_err)?;
                }
                "mode" => {
                    let vals: Vec<ByteArray> =
                        rows.iter().map(|r| ByteArray::from(r.mode)).collect();
                    writer
                        .typed::<ByteArrayType>()
                        .write_batch(&vals, None, None)
                        .map_err(pq_err)?;
                }
                "kind" => {
                    let vals: Vec<ByteArray> =
                        rows.iter().map(|r| ByteArray::from(r.kind)).collect();
                    writer
                        .typed::<ByteArrayType>()
                        .write_batch(&vals, None, None)
                        .map_err(pq_err)?;
                }
                "op" => {
                    let (vals, defs) = opt_str(&rows, |r| r.op.as_deref());
                    writer
                        .typed::<ByteArrayType>()
                        .write_batch(&vals, Some(&defs), None)
                        .map_err(pq_err)?;
                }
                "bucket" => {
                    let (vals, defs) = opt_str(&rows, |r| r.bucket.as_deref());
                    writer
                        .typed::<ByteArrayType>()
                        .write_batch(&vals, Some(&defs), None)
                        .map_err(pq_err)?;
                }
                "logical_bytes" => {
                    let (vals, defs) = opt_i64(&rows, |r| r.logical_bytes);
                    writer
                        .typed::<Int64Type>()
                        .write_batch(&vals, Some(&defs), None)
                        .map_err(pq_err)?;
                }
                "physical_bytes" => {
                    let (vals, defs) = opt_i64(&rows, |r| r.physical_bytes);
                    writer
                        .typed::<Int64Type>()
                        .write_batch(&vals, Some(&defs), None)
                        .map_err(pq_err)?;
                }
                "engine_put_requests" => {
                    let (vals, defs) = opt_i64(&rows, |r| r.engine_put_requests);
                    writer
                        .typed::<Int64Type>()
                        .write_batch(&vals, Some(&defs), None)
                        .map_err(pq_err)?;
                }
                "engine_get_requests" => {
                    let (vals, defs) = opt_i64(&rows, |r| r.engine_get_requests);
                    writer
                        .typed::<Int64Type>()
                        .write_batch(&vals, Some(&defs), None)
                        .map_err(pq_err)?;
                }
                "engine_bytes" => {
                    let (vals, defs) = opt_i64(&rows, |r| r.engine_bytes);
                    writer
                        .typed::<Int64Type>()
                        .write_batch(&vals, Some(&defs), None)
                        .map_err(pq_err)?;
                }
                "engine_cpu_us" => {
                    let (vals, defs) = opt_i64(&rows, |r| r.engine_cpu_us);
                    writer
                        .typed::<Int64Type>()
                        .write_batch(&vals, Some(&defs), None)
                        .map_err(pq_err)?;
                }
                "logical_stored_bytes" => {
                    let (vals, defs) = opt_i64(&rows, |r| r.logical_stored_bytes);
                    writer
                        .typed::<Int64Type>()
                        .write_batch(&vals, Some(&defs), None)
                        .map_err(pq_err)?;
                }
                "physical_stored_bytes" => {
                    let (vals, defs) = opt_i64(&rows, |r| r.physical_stored_bytes);
                    writer
                        .typed::<Int64Type>()
                        .write_batch(&vals, Some(&defs), None)
                        .map_err(pq_err)?;
                }
                "projected" => {
                    let (vals, defs) = opt_bool(&rows, |r| r.projected);
                    writer
                        .typed::<BoolType>()
                        .write_batch(&vals, Some(&defs), None)
                        .map_err(pq_err)?;
                }
                "packs_deleted" => {
                    let (vals, defs) = opt_i64(&rows, |r| r.packs_deleted);
                    writer
                        .typed::<Int64Type>()
                        .write_batch(&vals, Some(&defs), None)
                        .map_err(pq_err)?;
                }
                "bytes_freed" => {
                    let (vals, defs) = opt_i64(&rows, |r| r.bytes_freed);
                    writer
                        .typed::<Int64Type>()
                        .write_batch(&vals, Some(&defs), None)
                        .map_err(pq_err)?;
                }
                "chunks_freed" => {
                    let (vals, defs) = opt_i64(&rows, |r| r.chunks_freed);
                    writer
                        .typed::<Int64Type>()
                        .write_batch(&vals, Some(&defs), None)
                        .map_err(pq_err)?;
                }
                "dry_run" => {
                    let (vals, defs) = opt_bool(&rows, |r| r.dry_run);
                    writer
                        .typed::<BoolType>()
                        .write_batch(&vals, Some(&defs), None)
                        .map_err(pq_err)?;
                }
                "note" => {
                    let (vals, defs) = opt_str(&rows, |r| r.note.as_deref());
                    writer
                        .typed::<ByteArrayType>()
                        .write_batch(&vals, Some(&defs), None)
                        .map_err(pq_err)?;
                }
                "hash" => {
                    let vals: Vec<ByteArray> = rows
                        .iter()
                        .map(|r| ByteArray::from(r.hash.as_bytes().to_vec()))
                        .collect();
                    writer
                        .typed::<ByteArrayType>()
                        .write_batch(&vals, None, None)
                        .map_err(pq_err)?;
                }
                other => {
                    return Err(MeterError::Invalid(format!(
                        "unmapped parquet column {other}"
                    )))
                }
            }
        }
        col.close().map_err(pq_err)?;
        col_idx += 1;
    }
    rg.close().map_err(pq_err)?;
    writer.close().map_err(pq_err)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::GENESIS_PREV;
    use parquet::file::reader::{FileReader, SerializedFileReader};

    fn sample_records() -> Vec<MeterRecord> {
        let mk = |seq: u64, body: RecordBody| MeterRecord {
            seq,
            ts_ms: 1000 + seq,
            mode: Mode::Shadow,
            body,
            prev: String::new(),
            hash: String::new(),
        };
        let mut v = vec![
            mk(
                0,
                RecordBody::Op {
                    op: "put_object".into(),
                    bucket: "b".into(),
                    logical_bytes: 100,
                    physical_bytes: 40,
                    engine_put_requests: 2,
                    engine_get_requests: 0,
                    engine_bytes: 44,
                    engine_cpu_us: 7,
                },
            ),
            mk(
                1,
                RecordBody::Level {
                    logical_stored_bytes: 1000,
                    physical_stored_bytes: 200,
                    projected: true,
                },
            ),
            mk(
                2,
                RecordBody::GcSweep {
                    packs_deleted: 3,
                    bytes_freed: 999,
                    chunks_freed: 42,
                    dry_run: false,
                },
            ),
            mk(
                3,
                RecordBody::Note {
                    text: "hello".into(),
                },
            ),
        ];
        let mut prev = GENESIS_PREV.to_string();
        for r in v.iter_mut() {
            *r = r.clone().seal(&prev);
            prev = r.hash.clone();
        }
        v
    }

    #[test]
    fn csv_export_shape() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("meter.csv");
        export_csv(&sample_records(), &path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 5); // header + 4 rows
        assert!(lines[0].starts_with("seq,ts_ms,mode,kind"));
        assert!(lines[1].contains("put_object"));
        assert!(lines[4].contains("hello"));
    }

    #[test]
    fn parquet_export_readable() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("meter.parquet");
        export_parquet(&sample_records(), &path).unwrap();

        let reader = SerializedFileReader::new(File::open(&path).unwrap()).unwrap();
        assert_eq!(reader.metadata().file_metadata().num_rows(), 4);
        let mut iter = reader.get_row_iter(None).unwrap();
        let first = iter.next().unwrap().unwrap();
        let text = format!("{first:?}");
        assert!(text.contains("put_object"), "{text}");
    }
}
