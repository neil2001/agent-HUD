use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags};
use std::path::Path;
use std::time::Duration;

pub fn open_readonly(path: &Path) -> Result<Connection, rusqlite::Error> {
    let uri = format!("file:{}?mode=ro", path.to_string_lossy());
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY
        | OpenFlags::SQLITE_OPEN_URI
        | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let conn = Connection::open_with_flags(uri, flags)?;
    conn.busy_timeout(Duration::from_millis(2_000))?;
    let _ = conn.pragma_update(None, "query_only", true);
    Ok(conn)
}

pub fn row_opt_i64(row: &rusqlite::Row<'_>, idx: usize) -> Option<i64> {
    match row.get_ref(idx).ok()? {
        ValueRef::Integer(value) => Some(value),
        ValueRef::Real(value) => Some(value as i64),
        ValueRef::Text(bytes) => std::str::from_utf8(bytes).ok()?.parse().ok(),
        _ => None,
    }
}

pub fn row_bool(row: &rusqlite::Row<'_>, idx: usize) -> bool {
    match row.get_ref(idx).ok() {
        Some(ValueRef::Integer(value)) => value != 0,
        Some(ValueRef::Real(value)) => value != 0.0,
        Some(ValueRef::Text(bytes)) => matches!(
            std::str::from_utf8(bytes).unwrap_or_default(),
            "1" | "true" | "TRUE"
        ),
        _ => false,
    }
}

pub fn json_from_row(value: ValueRef<'_>) -> Result<serde_json::Value, rusqlite::Error> {
    json_from_value_ref(value)
}

fn json_from_value_ref(value: ValueRef<'_>) -> Result<serde_json::Value, rusqlite::Error> {
    let text = match value {
        ValueRef::Text(bytes) | ValueRef::Blob(bytes) => {
            std::str::from_utf8(bytes).map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Text,
                    Box::new(e),
                )
            })?
        }
        other => {
            return Err(rusqlite::Error::InvalidColumnType(
                0,
                other.data_type().to_string(),
                other.data_type(),
            ));
        }
    };
    serde_json::from_str(text).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
    })
}

pub fn read_item_json(
    conn: &Connection,
    key: &str,
) -> Result<Option<serde_json::Value>, rusqlite::Error> {
    let mut stmt = conn.prepare("SELECT value FROM ItemTable WHERE key = ?1 LIMIT 1")?;
    let mut rows = stmt.query([key])?;
    if let Some(row) = rows.next()? {
        return Ok(Some(json_from_value_ref(row.get_ref(0)?)?));
    }
    Ok(None)
}

pub fn read_disk_kv_json(
    conn: &Connection,
    key: &str,
) -> Result<Option<serde_json::Value>, rusqlite::Error> {
    let mut stmt = conn.prepare("SELECT value FROM cursorDiskKV WHERE key = ?1 LIMIT 1")?;
    let mut rows = stmt.query([key])?;
    if let Some(row) = rows.next()? {
        return Ok(Some(json_from_value_ref(row.get_ref(0)?)?));
    }
    Ok(None)
}
