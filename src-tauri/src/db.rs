use crate::commands::ShellState;
use rusqlite::{types::ValueRef, Connection, Row, ToSql};
use serde::Serialize;
use serde_json::{Number, Value};
use std::path::PathBuf;
use tauri::State;

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

#[tauri::command(rename_all = "camelCase")]
pub fn shell_db_query(
    state: State<'_, ShellState>,
    db_name: String,
    query: String,
    params: Option<Vec<Value>>,
) -> Result<QueryResult, String> {
    let connection = open_connection(&state, &db_name)?;
    let sql_params = sql_params(&params.unwrap_or_default())?;
    let mut statement = connection
        .prepare(&query)
        .map_err(|e| format!("prepare query: {e}"))?;

    let column_count = statement.column_count();
    let columns = (0..column_count)
        .map(|index| statement.column_name(index).map(|name| name.to_string()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("read columns: {e}"))?;

    let param_refs: Vec<&dyn ToSql> = sql_params.iter().map(|param| param as &dyn ToSql).collect();

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
}

#[tauri::command(rename_all = "camelCase")]
pub fn shell_db_execute(
    state: State<'_, ShellState>,
    db_name: String,
    query: String,
    params: Option<Vec<Value>>,
) -> Result<ExecuteResult, String> {
    let connection = open_connection(&state, &db_name)?;
    let sql_params = sql_params(&params.unwrap_or_default())?;
    let mut statement = connection
        .prepare(&query)
        .map_err(|e| format!("prepare query: {e}"))?;

    let param_refs: Vec<&dyn ToSql> = sql_params.iter().map(|param| param as &dyn ToSql).collect();

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
}
