//! export: CSV straight from the server, JSON from its NDJSON, Parquet built here from the CSV.

use std::io::{IsTerminal, Write};
use std::sync::Arc;

use hydronode_api::time::parse_instant_now;
use hydronode_api::{Error, Result};
use reqwest::Method;
use serde_json::Value;

use super::seg;
use crate::cli::{ExportArgs, ExportFormat, Resolution};
use crate::ctx::Ctx;

pub async fn run(ctx: &Ctx, args: ExportArgs) -> Result<()> {
    if args.format == ExportFormat::Parquet
        && args.output.is_none()
        && std::io::stdout().is_terminal()
    {
        return Err(Error::Usage(
            "Parquet is binary: write it to a file with -o readings.parquet".into(),
        ));
    }
    let from = parse_instant_now(&args.from)?;
    let to = parse_instant_now(&args.to)?;
    if from >= to {
        return Err(Error::Usage("--from must be before --to".into()));
    }
    let mut sensors = Vec::new();
    for s in &args.sensors {
        sensors.push(seg(s)?);
    }
    let resolution = match args.resolution {
        Resolution::Raw => "raw",
        Resolution::FiveMinutes => "5m",
        Resolution::Hour => "1h",
        Resolution::Day => "1d",
    };
    let wire = match args.format {
        ExportFormat::Json => "ndjson",
        _ => "csv",
    };
    let mut query = vec![
        ("from", from.to_string()),
        ("to", to.to_string()),
        ("format", wire.to_string()),
        ("resolution", resolution.to_string()),
    ];
    if !sensors.is_empty() {
        query.push(("sensors", sensors.join(",")));
    }
    let client = ctx.client()?;
    let raw = client
        .request(Method::GET, "/api/data-transfer/v1/export", &query, None)
        .await?;
    if !(200..300).contains(&raw.status) {
        return Err(hydronode_api::client::api_error(
            raw.status,
            &String::from_utf8_lossy(&raw.body),
        ));
    }
    let bytes = match args.format {
        ExportFormat::Csv => raw.body,
        ExportFormat::Json => {
            let rows: Vec<Value> = String::from_utf8_lossy(&raw.body)
                .lines()
                .filter(|l| !l.trim().is_empty())
                .map(|l| {
                    serde_json::from_str(l)
                        .map_err(|e| Error::Other(format!("bad export line: {e}")))
                })
                .collect::<Result<_>>()?;
            let mut text =
                serde_json::to_vec_pretty(&rows).map_err(|e| Error::Other(e.to_string()))?;
            text.push(b'\n');
            text
        }
        ExportFormat::Parquet => csv_to_parquet(&raw.body)?,
    };
    match &args.output {
        Some(path) => {
            std::fs::write(path, &bytes)?;
            eprintln!("Wrote {} ({} bytes).", path.display(), bytes.len());
        }
        None => {
            let mut out = std::io::stdout().lock();
            out.write_all(&bytes)?;
            out.flush()?;
        }
    }
    Ok(())
}

/// CSV with a header row → one Parquet row group. A column is DOUBLE when every non-empty cell
/// is a number, else UTF-8 text. Empty cells are null.
pub fn csv_to_parquet(csv_bytes: &[u8]) -> Result<Vec<u8>> {
    use parquet::basic::{Compression, LogicalType, Repetition, Type as Physical};
    use parquet::data_type::{ByteArray, ByteArrayType, DoubleType};
    use parquet::file::properties::WriterProperties;
    use parquet::file::writer::SerializedFileWriter;
    use parquet::schema::types::Type;

    let perr = |e: parquet::errors::ParquetError| Error::Other(format!("parquet: {e}"));
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(true)
        .from_reader(csv_bytes);
    let headers: Vec<String> = reader
        .headers()
        .map_err(|e| Error::Other(format!("csv: {e}")))?
        .iter()
        .map(str::to_string)
        .collect();
    let mut columns: Vec<Vec<String>> = vec![Vec::new(); headers.len()];
    for record in reader.records() {
        let record = record.map_err(|e| Error::Other(format!("csv: {e}")))?;
        for (i, col) in columns.iter_mut().enumerate() {
            col.push(record.get(i).unwrap_or("").to_string());
        }
    }
    let numeric: Vec<bool> = columns
        .iter()
        .map(|c| {
            c.iter().any(|x| !x.is_empty())
                && c.iter()
                    .all(|x| x.is_empty() || x.parse::<f64>().is_ok_and(f64::is_finite))
        })
        .collect();

    let fields = headers
        .iter()
        .zip(&numeric)
        .map(|(name, num)| {
            let builder = if *num {
                Type::primitive_type_builder(name, Physical::DOUBLE)
            } else {
                Type::primitive_type_builder(name, Physical::BYTE_ARRAY)
                    .with_logical_type(Some(LogicalType::String))
            };
            builder
                .with_repetition(Repetition::OPTIONAL)
                .build()
                .map(Arc::new)
                .map_err(perr)
        })
        .collect::<Result<Vec<_>>>()?;
    let schema = Arc::new(
        Type::group_type_builder("hydronode_export")
            .with_fields(fields)
            .build()
            .map_err(perr)?,
    );
    let props = Arc::new(
        WriterProperties::builder()
            .set_compression(Compression::UNCOMPRESSED)
            .set_created_by(concat!("hydronode-cli ", env!("CARGO_PKG_VERSION")).into())
            .build(),
    );
    let mut buf = Vec::new();
    let mut writer = SerializedFileWriter::new(&mut buf, schema, props).map_err(perr)?;
    let mut group = writer.next_row_group().map_err(perr)?;
    let mut index = 0;
    while let Some(mut col) = group.next_column().map_err(perr)? {
        let cells = &columns[index];
        let defs: Vec<i16> = cells.iter().map(|c| i16::from(!c.is_empty())).collect();
        if numeric[index] {
            let vals: Vec<f64> = cells
                .iter()
                .filter(|c| !c.is_empty())
                .map(|c| c.parse().unwrap_or(0.0))
                .collect();
            col.typed::<DoubleType>()
                .write_batch(&vals, Some(&defs), None)
                .map_err(perr)?;
        } else {
            let vals: Vec<ByteArray> = cells
                .iter()
                .filter(|c| !c.is_empty())
                .map(|c| ByteArray::from(c.as_str()))
                .collect();
            col.typed::<ByteArrayType>()
                .write_batch(&vals, Some(&defs), None)
                .map_err(perr)?;
        }
        col.close().map_err(perr)?;
        index += 1;
    }
    group.close().map_err(perr)?;
    writer.close().map_err(perr)?;
    Ok(buf)
}
