use std::io::{self, Write};
use std::sync::Arc;

use arrow_array::builder::{
    BinaryBuilder, BooleanBuilder, Date32Builder, Decimal128Builder, Float32Builder, Float64Builder, Int16Builder,
    Int32Builder, Int64Builder, StringBuilder, Time64MicrosecondBuilder, TimestampMicrosecondBuilder,
};
use arrow_array::{ArrayRef, RecordBatch};
use arrow_schema::{DataType, Field, Schema, SchemaRef, TimeUnit};
use parquet::arrow::ArrowWriter;
use parquet::basic::{Compression as Codec, GzipLevel};
use parquet::file::properties::WriterProperties;

use crate::export::{Compression, Settings};
use crate::transfer::{SqlType, instant};
use crate::value::{Column, Value};

const BATCH: usize = 4096;

enum Builder {
    Boolean(BooleanBuilder),
    Int16(Int16Builder),
    Int32(Int32Builder),
    Int64(Int64Builder),
    Float32(Float32Builder),
    Float64(Float64Builder),
    Decimal(Decimal128Builder, i8),
    Utf8(StringBuilder),
    Binary(BinaryBuilder),
    Date(Date32Builder),
    Time(Time64MicrosecondBuilder),
    Timestamp(TimestampMicrosecondBuilder, bool),
}

pub fn arrow_type(type_name: &str) -> DataType {
    match SqlType::parse(type_name) {
        SqlType::Boolean => DataType::Boolean,
        SqlType::SmallInt => DataType::Int16,
        SqlType::Integer => DataType::Int32,
        SqlType::BigInt => DataType::Int64,
        SqlType::Real => DataType::Float32,
        SqlType::Double => DataType::Float64,
        SqlType::Decimal { precision: Some(precision), scale: Some(scale) }
            if (1..=38).contains(&precision) && scale <= precision =>
        {
            DataType::Decimal128(precision as u8, scale as i8)
        }
        SqlType::Bytes => DataType::Binary,
        SqlType::Date => DataType::Date32,
        SqlType::Time => DataType::Time64(TimeUnit::Microsecond),
        SqlType::Timestamp => DataType::Timestamp(TimeUnit::Microsecond, None),
        SqlType::TimestampTz => DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
        _ => DataType::Utf8,
    }
}

fn builder(data_type: &DataType) -> Builder {
    match data_type {
        DataType::Boolean => Builder::Boolean(BooleanBuilder::new()),
        DataType::Int16 => Builder::Int16(Int16Builder::new()),
        DataType::Int32 => Builder::Int32(Int32Builder::new()),
        DataType::Int64 => Builder::Int64(Int64Builder::new()),
        DataType::Float32 => Builder::Float32(Float32Builder::new()),
        DataType::Float64 => Builder::Float64(Float64Builder::new()),
        DataType::Decimal128(precision, scale) => Builder::Decimal(
            Decimal128Builder::new()
                .with_precision_and_scale(*precision, *scale)
                .unwrap_or_else(|_| Decimal128Builder::new()),
            *scale,
        ),
        DataType::Binary => Builder::Binary(BinaryBuilder::new()),
        DataType::Date32 => Builder::Date(Date32Builder::new()),
        DataType::Time64(_) => Builder::Time(Time64MicrosecondBuilder::new()),
        DataType::Timestamp(_, Some(zone)) => {
            Builder::Timestamp(TimestampMicrosecondBuilder::new().with_timezone(Arc::clone(zone)), true)
        }
        DataType::Timestamp(_, None) => Builder::Timestamp(TimestampMicrosecondBuilder::new(), false),
        _ => Builder::Utf8(StringBuilder::new()),
    }
}

fn text_of(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::Array(_) | Value::Map(_) | Value::Document(_) => Some(crate::export::json_value(value)),
        other => other.display(),
    }
}

fn integer(value: &Value) -> Option<i64> {
    match value {
        Value::Integer(value) => Some(*value),
        Value::Bool(value) => Some(i64::from(*value)),
        other => other.display()?.trim().parse().ok(),
    }
}

fn float(value: &Value) -> Option<f64> {
    match value {
        Value::Float(value) => Some(*value),
        Value::Integer(value) => Some(*value as f64),
        other => other.display()?.trim().parse().ok(),
    }
}

pub fn decimal(text: &str, scale: i8) -> Option<i128> {
    let text = text.trim();
    let (negative, digits) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text.strip_prefix('+').unwrap_or(text)),
    };
    let (whole, fraction) = digits.split_once('.').unwrap_or((digits, ""));
    let scale = usize::try_from(scale).ok()?;
    let fraction = fraction.trim_end_matches('0');
    if (whole.is_empty() && fraction.is_empty())
        || fraction.len() > scale
        || !whole.chars().chain(fraction.chars()).all(|c| c.is_ascii_digit())
    {
        return None;
    }
    let mut number: i128 = 0;
    for c in whole.chars().chain(fraction.chars()).chain(std::iter::repeat_n('0', scale - fraction.len())) {
        number = number.checked_mul(10)?.checked_add(i128::from(c.to_digit(10)?))?;
    }
    Some(if negative { -number } else { number })
}

fn days(text: &str) -> Option<i32> {
    let date: jiff::civil::Date = text.trim().get(..10)?.parse().ok()?;
    let seconds = date.to_zoned(jiff::tz::TimeZone::UTC).ok()?.timestamp().as_second();
    i32::try_from(seconds.div_euclid(86_400)).ok()
}

fn time_of_day(text: &str) -> Option<i64> {
    let text = text.trim();
    let end = text.char_indices().skip(2).find(|(_, c)| matches!(c, '+' | '-')).map_or(text.len(), |(at, _)| at);
    let time: jiff::civil::Time = text[..end].parse().ok()?;
    i64::try_from(time.duration_since(jiff::civil::Time::midnight()).as_micros()).ok()
}

fn micros(text: &str, zoned: bool) -> Option<i64> {
    if zoned && let Some(instant) = instant(text) {
        return Some(instant.as_microsecond());
    }
    let datetime: jiff::civil::DateTime = text.trim().replacen(' ', "T", 1).parse().ok()?;
    Some(datetime.to_zoned(jiff::tz::TimeZone::UTC).ok()?.timestamp().as_microsecond())
}

impl Builder {
    fn append(&mut self, value: &Value) -> Result<(), ()> {
        if matches!(value, Value::Null) {
            self.append_null();
            return Ok(());
        }
        let text = || text_of(value).unwrap_or_default();
        match self {
            Builder::Boolean(builder) => builder.append_value(match value {
                Value::Bool(value) => *value,
                other => match (integer(other), text().trim().to_ascii_lowercase().as_str()) {
                    (Some(0), _) | (None, "false" | "f") => false,
                    (Some(1), _) | (None, "true" | "t") => true,
                    _ => return Err(()),
                },
            }),
            Builder::Int16(builder) => {
                builder.append_value(integer(value).and_then(|n| i16::try_from(n).ok()).ok_or(())?)
            }
            Builder::Int32(builder) => {
                builder.append_value(integer(value).and_then(|n| i32::try_from(n).ok()).ok_or(())?)
            }
            Builder::Int64(builder) => builder.append_value(integer(value).ok_or(())?),
            Builder::Float32(builder) => builder.append_value(float(value).ok_or(())? as f32),
            Builder::Float64(builder) => builder.append_value(float(value).ok_or(())?),
            Builder::Decimal(builder, scale) => builder.append_value(decimal(&text(), *scale).ok_or(())?),
            Builder::Utf8(builder) => builder.append_value(text()),
            Builder::Binary(builder) => match value {
                Value::Bytes(bytes) => builder.append_value(bytes),
                _ => builder.append_value(text().as_bytes()),
            },
            Builder::Date(builder) => builder.append_value(days(&text()).ok_or(())?),
            Builder::Time(builder) => builder.append_value(time_of_day(&text()).ok_or(())?),
            Builder::Timestamp(builder, zoned) => builder.append_value(micros(&text(), *zoned).ok_or(())?),
        }
        Ok(())
    }

    fn append_null(&mut self) {
        match self {
            Builder::Boolean(builder) => builder.append_null(),
            Builder::Int16(builder) => builder.append_null(),
            Builder::Int32(builder) => builder.append_null(),
            Builder::Int64(builder) => builder.append_null(),
            Builder::Float32(builder) => builder.append_null(),
            Builder::Float64(builder) => builder.append_null(),
            Builder::Decimal(builder, _) => builder.append_null(),
            Builder::Utf8(builder) => builder.append_null(),
            Builder::Binary(builder) => builder.append_null(),
            Builder::Date(builder) => builder.append_null(),
            Builder::Time(builder) => builder.append_null(),
            Builder::Timestamp(builder, _) => builder.append_null(),
        }
    }

    fn finish(&mut self) -> ArrayRef {
        match self {
            Builder::Boolean(builder) => Arc::new(builder.finish()),
            Builder::Int16(builder) => Arc::new(builder.finish()),
            Builder::Int32(builder) => Arc::new(builder.finish()),
            Builder::Int64(builder) => Arc::new(builder.finish()),
            Builder::Float32(builder) => Arc::new(builder.finish()),
            Builder::Float64(builder) => Arc::new(builder.finish()),
            Builder::Decimal(builder, _) => Arc::new(builder.finish()),
            Builder::Utf8(builder) => Arc::new(builder.finish()),
            Builder::Binary(builder) => Arc::new(builder.finish()),
            Builder::Date(builder) => Arc::new(builder.finish()),
            Builder::Time(builder) => Arc::new(builder.finish()),
            Builder::Timestamp(builder, _) => Arc::new(builder.finish()),
        }
    }
}

fn parquet_error(error: impl std::fmt::Display) -> io::Error {
    io::Error::other(error.to_string())
}

pub struct ParquetWriter<W: Write + Send> {
    writer: ArrowWriter<W>,
    schema: SchemaRef,
    columns: Vec<Column>,
    builders: Vec<Builder>,
    buffered: usize,
    rows: u64,
}

impl<W: Write + Send> ParquetWriter<W> {
    pub fn new(out: W, settings: &Settings, columns: Vec<Column>) -> io::Result<Self> {
        let fields: Vec<Field> =
            columns.iter().map(|column| Field::new(&column.name, arrow_type(&column.type_name), true)).collect();
        let schema: SchemaRef = Arc::new(Schema::new(fields));
        let codec = match settings.compression {
            Compression::None => Codec::UNCOMPRESSED,
            Compression::Snappy => Codec::SNAPPY,
            Compression::Gzip => Codec::GZIP(GzipLevel::default()),
            Compression::Lz4 => Codec::LZ4_RAW,
        };
        let properties = WriterProperties::builder()
            .set_compression(codec)
            .set_max_row_group_row_count(Some(settings.row_group.clamp(1_000, 1_000_000)))
            .set_created_by("Zen IDE".to_owned())
            .build();
        let writer = ArrowWriter::try_new(out, Arc::clone(&schema), Some(properties)).map_err(parquet_error)?;
        let builders = schema.fields().iter().map(|field| builder(field.data_type())).collect();
        Ok(Self { writer, schema, columns, builders, buffered: 0, rows: 0 })
    }

    pub fn rows(&self) -> u64 {
        self.rows
    }

    pub fn row(&mut self, row: &[Value]) -> io::Result<()> {
        for (at, builder) in self.builders.iter_mut().enumerate() {
            let value = row.get(at).unwrap_or(&Value::Null);
            if builder.append(value).is_err() {
                let column = &self.columns[at];
                return Err(io::Error::other(format!(
                    "the value of column {} does not fit its type {}",
                    column.name, column.type_name
                )));
            }
        }
        self.buffered += 1;
        self.rows += 1;
        if self.buffered >= BATCH {
            self.flush_batch()?;
        }
        Ok(())
    }

    fn flush_batch(&mut self) -> io::Result<()> {
        if self.buffered == 0 {
            return Ok(());
        }
        let arrays: Vec<ArrayRef> = self.builders.iter_mut().map(Builder::finish).collect();
        let batch = RecordBatch::try_new(Arc::clone(&self.schema), arrays).map_err(parquet_error)?;
        self.buffered = 0;
        self.writer.write(&batch).map_err(parquet_error)
    }

    pub fn finish(mut self) -> io::Result<W> {
        self.flush_batch()?;
        let mut out = self.writer.into_inner().map_err(parquet_error)?;
        out.flush()?;
        Ok(out)
    }
}

#[cfg(test)]
mod tests;
