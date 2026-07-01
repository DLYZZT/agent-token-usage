use crate::model::{CallStats, SessionStats, Usage};
use chrono::{DateTime, Local, Utc};
use serde::Serialize;
use std::collections::HashMap;
use std::env;
use std::path::{Path, PathBuf};

pub type UsageResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct UsageWithUncached {
    pub input_tokens: i64,
    pub cached_input_tokens: i64,
    pub output_tokens: i64,
    pub reasoning_output_tokens: i64,
    pub total_tokens: i64,
    pub uncached_input_tokens: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Summary {
    pub sessions: usize,
    pub calls: usize,
    pub start: Option<String>,
    pub end: Option<String>,
    pub usage: UsageWithUncached,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct SessionJson {
    path: String,
    session_id: Option<String>,
    start: Option<String>,
    end: Option<String>,
    cwd: Option<String>,
    model: Option<String>,
    provider: Option<String>,
    cli_version: Option<String>,
    context_window: Option<i64>,
    calls: usize,
    usage: UsageWithUncached,
    malformed_lines: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct JsonPayload {
    summary: Summary,
    sessions: Vec<SessionJson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    calls: Option<Vec<HashMap<&'static str, CellValue>>>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(untagged)]
enum CellValue {
    Int(i64),
    Text(String),
}

pub fn aggregate_sessions(sessions: &[SessionStats]) -> Summary {
    let usage = sessions
        .iter()
        .fold(Usage::default(), |usage, session| usage.add(&session.usage));
    let start = sessions
        .iter()
        .filter_map(|session| session.start)
        .min()
        .map(|value| value.to_rfc3339());
    let end = sessions
        .iter()
        .filter_map(|session| session.end)
        .max()
        .map(|value| value.to_rfc3339());

    Summary {
        sessions: sessions.len(),
        calls: sessions.iter().map(SessionStats::call_count).sum(),
        start,
        end,
        usage: usage_with_uncached(&usage),
    }
}

pub fn render_summary(sessions: &[SessionStats], scope: &str, source: &str) -> String {
    let summary = aggregate_sessions(sessions);
    let usage = &summary.usage;
    format!(
        "{} token usage ({})\nSessions: {}    Calls: {}\nPeriod:   {} -> {}\nInput:    {}\nCached:   {}\nUncached: {}\nOutput:   {}\nReason:   {}\nTotal:    {}\n",
        capitalize(source),
        scope,
        summary.sessions,
        summary.calls,
        format_dt(
            summary
                .start
                .as_deref()
                .and_then(|value| parse_rfc3339(value))
        ),
        format_dt(
            summary
                .end
                .as_deref()
                .and_then(|value| parse_rfc3339(value))
        ),
        format_int(usage.input_tokens),
        format_int(usage.cached_input_tokens),
        format_int(usage.uncached_input_tokens),
        format_int(usage.output_tokens),
        format_int(usage.reasoning_output_tokens),
        format_int(usage.total_tokens),
    )
}

pub fn render_json(sessions: &[SessionStats], include_calls: bool) -> UsageResult<String> {
    let payload = JsonPayload {
        summary: aggregate_sessions(sessions),
        sessions: sessions.iter().map(session_json).collect(),
        calls: include_calls.then(|| {
            sessions
                .iter()
                .flat_map(|session| {
                    session
                        .calls
                        .iter()
                        .map(|call| call_row(session, call))
                        .collect::<Vec<_>>()
                })
                .collect()
        }),
    };
    Ok(format!("{}\n", serde_json::to_string_pretty(&payload)?))
}

pub fn render_summary_csv(sessions: &[SessionStats], scope: &str) -> UsageResult<String> {
    let summary = aggregate_sessions(sessions);
    let mut row = HashMap::new();
    row.insert("scope", CellValue::Text(scope.to_string()));
    row.insert("sessions", CellValue::Int(summary.sessions as i64));
    row.insert("calls", CellValue::Int(summary.calls as i64));
    row.insert("start", CellValue::Text(summary.start.unwrap_or_default()));
    row.insert("end", CellValue::Text(summary.end.unwrap_or_default()));
    row.insert("input_tokens", CellValue::Int(summary.usage.input_tokens));
    row.insert(
        "cached_input_tokens",
        CellValue::Int(summary.usage.cached_input_tokens),
    );
    row.insert(
        "uncached_input_tokens",
        CellValue::Int(summary.usage.uncached_input_tokens),
    );
    row.insert("output_tokens", CellValue::Int(summary.usage.output_tokens));
    row.insert(
        "reasoning_output_tokens",
        CellValue::Int(summary.usage.reasoning_output_tokens),
    );
    row.insert("total_tokens", CellValue::Int(summary.usage.total_tokens));
    write_csv(
        &[row],
        &[
            "scope",
            "sessions",
            "calls",
            "start",
            "end",
            "input_tokens",
            "cached_input_tokens",
            "uncached_input_tokens",
            "output_tokens",
            "reasoning_output_tokens",
            "total_tokens",
        ],
    )
}

pub fn render_sessions_csv(sessions: &[SessionStats]) -> UsageResult<String> {
    let rows = sessions.iter().map(session_row).collect::<Vec<_>>();
    write_csv(
        &rows,
        &[
            "start",
            "end",
            "session_id",
            "calls",
            "input_tokens",
            "cached_input_tokens",
            "uncached_input_tokens",
            "output_tokens",
            "reasoning_output_tokens",
            "total_tokens",
            "context_window",
            "model",
            "provider",
            "cwd",
            "file",
            "malformed_lines",
        ],
    )
}

pub fn render_calls_csv(sessions: &[SessionStats]) -> UsageResult<String> {
    let rows = sessions
        .iter()
        .flat_map(|session| {
            session
                .calls
                .iter()
                .map(|call| call_row(session, call))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    write_csv(
        &rows,
        &[
            "time",
            "session_id",
            "input_tokens",
            "cached_input_tokens",
            "uncached_input_tokens",
            "output_tokens",
            "reasoning_output_tokens",
            "total_tokens",
            "running_total_tokens",
            "model",
            "provider",
            "file",
        ],
    )
}

pub fn sessions_table(sessions: &[SessionStats]) -> UsageResult<String> {
    let rows = sessions.iter().map(session_row).collect::<Vec<_>>();
    Ok(print_table(
        &rows,
        &[
            ("start", "START"),
            ("calls", "CALLS"),
            ("total_tokens", "TOTAL"),
            ("input_tokens", "INPUT"),
            ("cached_input_tokens", "CACHED"),
            ("output_tokens", "OUTPUT"),
            ("reasoning_output_tokens", "REASON"),
            ("model", "MODEL"),
            ("provider", "PROVIDER"),
            ("cwd", "CWD"),
            ("file", "FILE"),
        ],
    ))
}

pub fn calls_table(sessions: &[SessionStats], sort_key: &str, limit: i64) -> UsageResult<String> {
    let mut rows = sessions
        .iter()
        .flat_map(|session| {
            session
                .calls
                .iter()
                .map(|call| call_row(session, call))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    if sort_key == "tokens" {
        rows.sort_by(|left, right| {
            cell_int(right, "total_tokens").cmp(&cell_int(left, "total_tokens"))
        });
    }
    if limit > 0 {
        rows.truncate(limit as usize);
    }
    Ok(print_table(
        &rows,
        &[
            ("time", "TIME"),
            ("total_tokens", "TOTAL"),
            ("input_tokens", "INPUT"),
            ("cached_input_tokens", "CACHED"),
            ("output_tokens", "OUTPUT"),
            ("reasoning_output_tokens", "REASON"),
            ("model", "MODEL"),
            ("provider", "PROVIDER"),
            ("file", "FILE"),
        ],
    ))
}

fn session_json(session: &SessionStats) -> SessionJson {
    SessionJson {
        path: session.path.to_string_lossy().to_string(),
        session_id: session.session_id.clone(),
        start: session.start.map(|value| value.to_rfc3339()),
        end: session.end.map(|value| value.to_rfc3339()),
        cwd: session.cwd.clone(),
        model: session.model.clone(),
        provider: session.provider.clone(),
        cli_version: session.cli_version.clone(),
        context_window: session.context_window,
        calls: session.call_count(),
        usage: usage_with_uncached(&session.usage),
        malformed_lines: session.malformed_lines,
    }
}

fn session_row(session: &SessionStats) -> HashMap<&'static str, CellValue> {
    let mut row = HashMap::new();
    row.insert("start", CellValue::Text(format_dt(session.start)));
    row.insert("end", CellValue::Text(format_dt(session.end)));
    row.insert(
        "session_id",
        CellValue::Text(
            session
                .session_id
                .clone()
                .unwrap_or_else(|| "-".to_string()),
        ),
    );
    row.insert("calls", CellValue::Int(session.call_count() as i64));
    row.insert("input_tokens", CellValue::Int(session.usage.input_tokens));
    row.insert(
        "cached_input_tokens",
        CellValue::Int(session.usage.cached_input_tokens),
    );
    row.insert(
        "uncached_input_tokens",
        CellValue::Int(session.usage.uncached_input_tokens()),
    );
    row.insert("output_tokens", CellValue::Int(session.usage.output_tokens));
    row.insert(
        "reasoning_output_tokens",
        CellValue::Int(session.usage.reasoning_output_tokens),
    );
    row.insert("total_tokens", CellValue::Int(session.usage.total_tokens));
    row.insert("context_window", optional_int_text(session.context_window));
    row.insert(
        "model",
        CellValue::Text(session.model.clone().unwrap_or_else(|| "-".to_string())),
    );
    row.insert(
        "provider",
        CellValue::Text(session.provider.clone().unwrap_or_else(|| "-".to_string())),
    );
    row.insert(
        "cwd",
        CellValue::Text(session.cwd.clone().unwrap_or_else(|| "-".to_string())),
    );
    row.insert("file", CellValue::Text(compact_path(&session.path)));
    row.insert(
        "malformed_lines",
        CellValue::Int(session.malformed_lines as i64),
    );
    row
}

fn call_row(session: &SessionStats, call: &CallStats) -> HashMap<&'static str, CellValue> {
    let mut row = HashMap::new();
    row.insert("time", CellValue::Text(format_dt(call.timestamp)));
    row.insert(
        "session_id",
        CellValue::Text(
            session
                .session_id
                .clone()
                .unwrap_or_else(|| "-".to_string()),
        ),
    );
    row.insert("input_tokens", CellValue::Int(call.usage.input_tokens));
    row.insert(
        "cached_input_tokens",
        CellValue::Int(call.usage.cached_input_tokens),
    );
    row.insert(
        "uncached_input_tokens",
        CellValue::Int(call.usage.uncached_input_tokens()),
    );
    row.insert("output_tokens", CellValue::Int(call.usage.output_tokens));
    row.insert(
        "reasoning_output_tokens",
        CellValue::Int(call.usage.reasoning_output_tokens),
    );
    row.insert("total_tokens", CellValue::Int(call.usage.total_tokens));
    row.insert(
        "running_total_tokens",
        CellValue::Int(call.running_total.total_tokens),
    );
    row.insert(
        "model",
        CellValue::Text(session.model.clone().unwrap_or_else(|| "-".to_string())),
    );
    row.insert(
        "provider",
        CellValue::Text(session.provider.clone().unwrap_or_else(|| "-".to_string())),
    );
    row.insert("file", CellValue::Text(compact_path(&session.path)));
    row
}

fn write_csv(
    rows: &[HashMap<&'static str, CellValue>],
    fieldnames: &[&'static str],
) -> UsageResult<String> {
    let mut writer = csv::Writer::from_writer(Vec::new());
    writer.write_record(fieldnames)?;
    for row in rows {
        let record = fieldnames
            .iter()
            .map(|field| cell_text(row, field))
            .collect::<Vec<_>>();
        writer.write_record(record)?;
    }
    let bytes = writer.into_inner()?;
    Ok(String::from_utf8(bytes)?)
}

fn print_table(rows: &[HashMap<&'static str, CellValue>], columns: &[(&str, &str)]) -> String {
    if rows.is_empty() {
        return "No rows.\n".to_string();
    }

    let widths = columns
        .iter()
        .map(|(key, heading)| {
            let width = rows
                .iter()
                .map(|row| cell_text(row, key).len())
                .chain(std::iter::once(heading.len()))
                .max()
                .unwrap_or(0);
            (*key, width)
        })
        .collect::<HashMap<_, _>>();

    let mut output = String::new();
    output.push_str(
        &columns
            .iter()
            .map(|(key, heading)| pad_right(heading, widths[key]))
            .collect::<Vec<_>>()
            .join("  "),
    );
    output.push('\n');
    output.push_str(
        &columns
            .iter()
            .map(|(key, _)| "-".repeat(widths[key]))
            .collect::<Vec<_>>()
            .join("  "),
    );
    output.push('\n');
    for row in rows {
        output.push_str(
            &columns
                .iter()
                .map(|(key, _)| match row.get(key) {
                    Some(CellValue::Int(value)) => pad_left(&format_int(*value), widths[key]),
                    _ => pad_right(&cell_text(row, key), widths[key]),
                })
                .collect::<Vec<_>>()
                .join("  "),
        );
        output.push('\n');
    }
    output
}

fn usage_with_uncached(usage: &Usage) -> UsageWithUncached {
    UsageWithUncached {
        input_tokens: usage.input_tokens,
        cached_input_tokens: usage.cached_input_tokens,
        output_tokens: usage.output_tokens,
        reasoning_output_tokens: usage.reasoning_output_tokens,
        total_tokens: usage.total_tokens,
        uncached_input_tokens: usage.uncached_input_tokens(),
    }
}

fn optional_int_text(value: Option<i64>) -> CellValue {
    value
        .map(CellValue::Int)
        .unwrap_or_else(|| CellValue::Text("-".to_string()))
}

fn cell_text(row: &HashMap<&'static str, CellValue>, key: &str) -> String {
    match row.get(key) {
        Some(CellValue::Int(value)) => value.to_string(),
        Some(CellValue::Text(value)) => value.clone(),
        None => String::new(),
    }
}

fn cell_int(row: &HashMap<&'static str, CellValue>, key: &str) -> i64 {
    match row.get(key) {
        Some(CellValue::Int(value)) => *value,
        Some(CellValue::Text(value)) => value.parse().unwrap_or(0),
        None => 0,
    }
}

fn format_dt(value: Option<DateTime<Utc>>) -> String {
    value
        .map(|value| {
            value
                .with_timezone(&Local)
                .format("%Y-%m-%d %H:%M:%S")
                .to_string()
        })
        .unwrap_or_else(|| "-".to_string())
}

fn parse_rfc3339(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|value| value.with_timezone(&Utc))
}

fn format_int(value: i64) -> String {
    let negative = value < 0;
    let digits = value.abs().to_string();
    let mut formatted = String::new();
    for (index, ch) in digits.chars().rev().enumerate() {
        if index > 0 && index % 3 == 0 {
            formatted.push(',');
        }
        formatted.push(ch);
    }
    let mut formatted = formatted.chars().rev().collect::<String>();
    if negative {
        formatted.insert(0, '-');
    }
    formatted
}

fn compact_path(path: &Path) -> String {
    let home = env::var_os("HOME").map(PathBuf::from);
    let resolved = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if let Some(home) = home {
        if let Ok(relative) = resolved.strip_prefix(home) {
            return format!("~/{}", relative.to_string_lossy());
        }
    }
    path.to_string_lossy().to_string()
}

fn capitalize(value: &str) -> String {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

fn pad_left(value: &str, width: usize) -> String {
    format!("{value:>width$}")
}

fn pad_right(value: &str, width: usize) -> String {
    format!("{value:<width$}")
}
