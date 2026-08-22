use crate::commands::ShellState;
use rusqlite::{types::ValueRef, Connection, Row, ToSql};
use serde::Serialize;
use serde_json::{Number, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use tauri::State;

/// Idle cached connections are closed after this long without a query/execute.
const DEFAULT_IDLE_TIMEOUT: Duration = Duration::from_secs(30);
const REAPER_INTERVAL: Duration = Duration::from_secs(5);

struct CachedConn {
    conn: Connection,
    last_used: Instant,
}

struct Inner {
    connections: HashMap<String, CachedConn>,
    idle_timeout: Duration,
}

/// Process-wide SQLite connection cache. Connections stay open for reuse
/// until `dbClose` or the idle timeout.
pub struct DbState {
    inner: Arc<Mutex<Inner>>,
}

impl DbState {
    pub fn new() -> Self {
        Self::spawn(DEFAULT_IDLE_TIMEOUT, true)
    }

    fn spawn(idle_timeout: Duration, start_reaper: bool) -> Self {
        let inner = Arc::new(Mutex::new(Inner {
            connections: HashMap::new(),
            idle_timeout,
        }));
        if start_reaper {
            let reaper = inner.clone();
            let _ = thread::Builder::new()
                .name("shell-db-reaper".into())
                .spawn(move || loop {
                    thread::sleep(REAPER_INTERVAL);
                    reap_idle(&reaper);
                });
        }
        DbState { inner }
    }
}

#[derive(Debug, Serialize)]
pub struct QueryResult {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Value>>,
}

#[derive(Debug, Serialize)]
pub struct ExecuteResult {
    pub changes: u64,
    pub last_insert_rowid: i64,
}

enum SqlParam {
    Null,
    Integer(i64),
    Real(f64),
    Text(String),
}

impl ToSql for SqlParam {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        match self {
            SqlParam::Null => Ok(rusqlite::types::Null.into()),
            SqlParam::Integer(value) => Ok((*value).into()),
            SqlParam::Real(value) => Ok((*value).into()),
            SqlParam::Text(value) => Ok(value.as_str().into()),
        }
    }
}

fn validate_db_name(name: &str) -> Result<(), String> {
    if name.is_empty()
        || name.contains('/')
        || name.contains('\\')
        || name.contains("..")
        || name.contains('\0')
    {
        return Err("invalid database name".into());
    }
    Ok(())
}

fn db_path(state: &ShellState, name: &str) -> Result<PathBuf, String> {
    validate_db_name(name)?;
    let path = state.data_root.join(name);
    if !path.starts_with(&state.data_root) {
        return Err("path escape".into());
    }
    Ok(path)
}

fn open_connection(state: &ShellState, name: &str) -> Result<Connection, String> {
    let path = db_path(state, name)?;
    Connection::open(path).map_err(|e| format!("open database: {e}"))
}

fn lock_inner(db: &DbState) -> Result<std::sync::MutexGuard<'_, Inner>, String> {
    db.inner
        .lock()
        .map_err(|_| "database state lock poisoned".to_string())
}

fn close_connection(conn: Connection) {
    let _ = conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
    let _ = conn.close();
}

fn with_connection<T>(
    db: &DbState,
    shell: &ShellState,
    name: &str,
    f: impl FnOnce(&Connection) -> Result<T, String>,
) -> Result<T, String> {
    let mut inner = lock_inner(db)?;
    if !inner.connections.contains_key(name) {
        let conn = open_connection(shell, name)?;
        inner.connections.insert(
            name.to_string(),
            CachedConn {
                conn,
                last_used: Instant::now(),
            },
        );
    }
    let cached = inner
        .connections
        .get_mut(name)
        .ok_or_else(|| "database connection missing".to_string())?;
    cached.last_used = Instant::now();
    f(&cached.conn)
}

fn close_cached(db: &DbState, db_name: Option<&str>) -> Result<(), String> {
    let mut inner = lock_inner(db)?;
    match db_name {
        Some(name) => {
            validate_db_name(name)?;
            if let Some(cached) = inner.connections.remove(name) {
                close_connection(cached.conn);
            }
        }
        None => {
            let leftover = std::mem::take(&mut inner.connections);
            for (_, cached) in leftover {
                close_connection(cached.conn);
            }
        }
    }
    Ok(())
}

fn reap_idle(inner: &Mutex<Inner>) {
    let Ok(mut guard) = inner.lock() else {
        return;
    };
    let timeout = guard.idle_timeout;
    let now = Instant::now();
    let stale: Vec<String> = guard
        .connections
        .iter()
        .filter(|(_, cached)| now.duration_since(cached.last_used) >= timeout)
        .map(|(name, _)| name.clone())
        .collect();
    for name in stale {
        if let Some(cached) = guard.connections.remove(&name) {
            close_connection(cached.conn);
        }
    }
}

fn sql_params(params: &[Value]) -> Result<Vec<SqlParam>, String> {
    params
        .iter()
        .map(|param| match param {
            Value::Null => Ok(SqlParam::Null),
            Value::Bool(value) => Ok(SqlParam::Integer(if *value { 1 } else { 0 })),
            Value::Number(number) => {
                if let Some(integer) = number.as_i64() {
                    Ok(SqlParam::Integer(integer))
                } else if let Some(float) = number.as_f64() {
                    Ok(SqlParam::Real(float))
                } else {
                    Err("invalid number parameter".into())
                }
            }
            Value::String(text) => Ok(SqlParam::Text(text.clone())),
            Value::Array(_) | Value::Object(_) => Err("unsupported parameter type".into()),
        })
        .collect()
}

fn value_from_row(row: &Row<'_>, index: usize) -> Result<Value, String> {
    match row
        .get_ref(index)
        .map_err(|e| format!("read column: {e}"))?
    {
        ValueRef::Null => Ok(Value::Null),
        ValueRef::Integer(value) => Ok(Value::Number(value.into())),
        ValueRef::Real(value) => Ok(Number::from_f64(value)
            .map(Value::Number)
            .unwrap_or(Value::Null)),
        ValueRef::Text(value) => Ok(Value::String(String::from_utf8_lossy(value).into_owned())),
        ValueRef::Blob(_) => Ok(Value::Null),
    }
}

pub fn db_query(
    shell: &ShellState,
    db: &DbState,
    db_name: String,
    query: String,
    params: Option<Vec<Value>>,
) -> Result<QueryResult, String> {
    let sql_params = sql_params(&params.unwrap_or_default())?;
    with_connection(db, shell, &db_name, |connection| {
        let mut statement = connection
            .prepare(&query)
            .map_err(|e| format!("prepare query: {e}"))?;

        let column_count = statement.column_count();
        let columns = (0..column_count)
            .map(|index| statement.column_name(index).map(|name| name.to_string()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("read columns: {e}"))?;

        let param_refs: Vec<&dyn ToSql> =
            sql_params.iter().map(|param| param as &dyn ToSql).collect();

        let mut rows = Vec::new();
        let mut row_iterator = statement
            .query(param_refs.as_slice())
            .map_err(|e| format!("execute query: {e}"))?;

        while let Some(row) = row_iterator.next().map_err(|e| format!("read row: {e}"))? {
            let mut values = Vec::with_capacity(column_count);
            for index in 0..column_count {
                values.push(value_from_row(row, index)?);
            }
            rows.push(values);
        }

        Ok(QueryResult { columns, rows })
    })
}

#[tauri::command(rename_all = "camelCase")]
pub fn shell_db_query(
    state: State<'_, ShellState>,
    db: State<'_, DbState>,
    db_name: String,
    query: String,
    params: Option<Vec<Value>>,
) -> Result<QueryResult, String> {
    db_query(&state, &db, db_name, query, params)
}

pub fn db_execute(
    shell: &ShellState,
    db: &DbState,
    db_name: String,
    query: String,
    params: Option<Vec<Value>>,
) -> Result<ExecuteResult, String> {
    let sql_params = sql_params(&params.unwrap_or_default())?;
    with_connection(db, shell, &db_name, |connection| {
        let mut statement = connection
            .prepare(&query)
            .map_err(|e| format!("prepare query: {e}"))?;

        let param_refs: Vec<&dyn ToSql> =
            sql_params.iter().map(|param| param as &dyn ToSql).collect();

        statement
            .execute(param_refs.as_slice())
            .map_err(|e| format!("execute query: {e}"))?;

        Ok(ExecuteResult {
            changes: connection
                .changes()
                .try_into()
                .map_err(|_| "changes count overflow".to_string())?,
            last_insert_rowid: connection.last_insert_rowid(),
        })
    })
}

#[tauri::command(rename_all = "camelCase")]
pub fn shell_db_execute(
    state: State<'_, ShellState>,
    db: State<'_, DbState>,
    db_name: String,
    query: String,
    params: Option<Vec<Value>>,
) -> Result<ExecuteResult, String> {
    db_execute(&state, &db, db_name, query, params)
}

pub fn db_close(db: &DbState, db_name: Option<&str>) -> Result<(), String> {
    close_cached(db, db_name)
}

#[tauri::command(rename_all = "camelCase")]
pub fn shell_db_close(db: State<'_, DbState>, db_name: Option<String>) -> Result<(), String> {
    db_close(&db, db_name.as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::ShellState;
    use std::path::PathBuf;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "app-ly-db-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn setup_with_timeout(idle: Duration) -> (PathBuf, ShellState, DbState) {
        let dir = temp_dir();
        let shell = ShellState {
            data_root: dir.clone(),
        };
        let db = DbState::spawn(idle, false);
        (dir, shell, db)
    }

    fn cached_count(db: &DbState) -> usize {
        db.inner.lock().map(|g| g.connections.len()).unwrap_or(0)
    }

    #[test]
    fn query_caches_connection_until_close() {
        let (dir, shell, db) = setup_with_timeout(Duration::from_secs(30));
        with_connection(&db, &shell, "app.db", |conn| {
            conn.execute_batch("CREATE TABLE t (id INTEGER); INSERT INTO t VALUES (1);")
                .map_err(|e| e.to_string())
        })
        .unwrap();
        with_connection(&db, &shell, "app.db", |_| Ok(())).unwrap();
        assert_eq!(cached_count(&db), 1);

        close_cached(&db, Some("app.db")).unwrap();
        assert_eq!(cached_count(&db), 0);
        std::fs::remove_file(dir.join("app.db")).expect("close must free the db file");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn close_without_name_closes_all() {
        let (dir, shell, db) = setup_with_timeout(Duration::from_secs(30));
        with_connection(&db, &shell, "a.db", |_| Ok(())).unwrap();
        with_connection(&db, &shell, "b.db", |_| Ok(())).unwrap();
        assert_eq!(cached_count(&db), 2);

        close_cached(&db, None).unwrap();
        assert_eq!(cached_count(&db), 0);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn close_unknown_name_is_ok() {
        let (dir, _shell, db) = setup_with_timeout(Duration::from_secs(30));
        close_cached(&db, Some("missing.db")).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn idle_timeout_closes_connection() {
        let (dir, shell, db) = setup_with_timeout(Duration::from_millis(20));
        with_connection(&db, &shell, "app.db", |_| Ok(())).unwrap();
        assert_eq!(cached_count(&db), 1);

        std::thread::sleep(Duration::from_millis(30));
        reap_idle(&db.inner);
        assert_eq!(cached_count(&db), 0);
        std::fs::remove_file(dir.join("app.db")).expect("idle close must free the db file");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
