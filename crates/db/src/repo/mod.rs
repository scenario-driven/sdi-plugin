//! Repository functions. Each module wraps one entity table; all functions
//! take a `&Connection` (or `&Transaction`) so that callers can compose them
//! into pool::tx() blocks for atomic multi-statement work.

pub mod agent_note;
pub mod agent_spec;
pub mod autonomy_policy;
pub mod collab;
pub mod decision;
pub mod disruption;
pub mod event;
pub mod knowledge;
pub mod pattern;
pub mod plan;
pub mod project;
pub mod requirement;
pub mod round;
pub mod run;
pub mod scenario;
pub mod task;
pub mod usage;

use chrono::{DateTime, Utc};
use rusqlite::types::FromSql;
use rusqlite::Row;

/// Decode an RFC3339 timestamp column as DateTime<Utc>.
pub(crate) fn ts(row: &Row<'_>, idx: usize) -> rusqlite::Result<DateTime<Utc>> {
    let raw: String = row.get(idx)?;
    DateTime::parse_from_rfc3339(&raw)
        .map(|d| d.with_timezone(&Utc))
        .map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(idx, rusqlite::types::Type::Text, Box::new(e))
        })
}

/// Decode an optional RFC3339 timestamp column.
pub(crate) fn ts_opt(row: &Row<'_>, idx: usize) -> rusqlite::Result<Option<DateTime<Utc>>> {
    let raw: Option<String> = row.get(idx)?;
    raw.map(|s| {
        DateTime::parse_from_rfc3339(&s)
            .map(|d| d.with_timezone(&Utc))
            .map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    idx,
                    rusqlite::types::Type::Text,
                    Box::new(e),
                )
            })
    })
    .transpose()
}

/// Decode a column that should parse via FromStr through `String`.
pub(crate) fn parsed<T>(row: &Row<'_>, idx: usize) -> rusqlite::Result<T>
where
    T: std::str::FromStr,
    T::Err: std::error::Error + Send + Sync + 'static,
{
    let raw: String = row.get(idx)?;
    raw.parse::<T>().map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(idx, rusqlite::types::Type::Text, Box::new(e))
    })
}

/// Convenience: read a TEXT column as a String.
pub(crate) fn s(row: &Row<'_>, idx: usize) -> rusqlite::Result<String> {
    row.get(idx)
}

/// Read an optional TEXT column.
pub(crate) fn s_opt(row: &Row<'_>, idx: usize) -> rusqlite::Result<Option<String>> {
    row.get(idx)
}

/// Read a JSON-string column and parse it. Used by B4 entities.
#[allow(dead_code)]
pub(crate) fn json<T: serde::de::DeserializeOwned>(
    row: &Row<'_>,
    idx: usize,
) -> rusqlite::Result<T> {
    let raw: String = row.get(idx)?;
    serde_json::from_str(&raw).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(idx, rusqlite::types::Type::Text, Box::new(e))
    })
}

/// Format a timestamp as RFC3339 with fractional seconds (UTC).
pub(crate) fn fmt_ts(t: DateTime<Utc>) -> String {
    t.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// Decode a value via T: FromSql (kept for parity with future extensions).
#[allow(dead_code)]
pub(crate) fn val<T: FromSql>(row: &Row<'_>, idx: usize) -> rusqlite::Result<T> {
    row.get(idx)
}
