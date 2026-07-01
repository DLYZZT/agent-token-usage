use crate::discovery::{is_grok_session_dir, is_opencode_db};
use crate::model::{CallStats, SessionStats, Usage};
use crate::time::parse_timestamp;
use chrono::{TimeZone, Utc};
use rusqlite::{Connection, Row};
use serde_json::Value;
use std::collections::HashMap;
use std::collections::HashSet;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

pub type UsageResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

pub fn detect_format(path: &Path) -> UsageResult<String> {
    if path.is_dir() {
        return Ok(if is_grok_session_dir(path) {
            "grok".to_string()
        } else {
            "codex".to_string()
        });
    }

    let mut candidate_format = None;
    let handle = File::open(path)?;
    for line in BufReader::new(handle).lines() {
        let line = line?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(event) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let event_type = event.get("type").and_then(Value::as_str);
        match event_type {
            Some("session_meta" | "turn_context" | "event_msg") => return Ok("codex".to_string()),
            Some("assistant" | "user") if event.get("message").is_some() => {
                return Ok("claude".to_string());
            }
            Some("session") if event.get("version").and_then(Value::as_i64) == Some(3) => {
                candidate_format = Some("pi".to_string());
            }
            Some("message") if event.get("message").is_some_and(Value::is_object) => {
                let message = event.get("message").unwrap();
                if message.get("api").is_some() {
                    return Ok("openclaw".to_string());
                }
                return Ok("pi".to_string());
            }
            _ => {}
        }
    }
    Ok(candidate_format.unwrap_or_else(|| "codex".to_string()))
}

pub fn parse_session_files(path: &Path) -> UsageResult<Vec<SessionStats>> {
    if is_opencode_db(path) {
        parse_opencode_db(path)
    } else {
        Ok(vec![parse_session_file(path)?])
    }
}

pub fn parse_session_file(path: &Path) -> UsageResult<SessionStats> {
    match detect_format(path)?.as_str() {
        "grok" => parse_grok_dir(path),
        "claude" => parse_claude_file(path),
        "openclaw" => parse_openclaw_file(path),
        "pi" => parse_pi_file(path, pi_usage_to_fields),
        _ => parse_codex_file(path),
    }
}

pub fn parse_codex_file(path: &Path) -> UsageResult<SessionStats> {
    let mut stats = SessionStats::new(path.to_path_buf());
    let mut previous_total_key = None;
    let mut previous_total_usage = None;

    read_jsonl(path, |event, malformed| {
        if malformed {
            stats.malformed_lines += 1;
            return;
        }

        let timestamp = parse_timestamp(value_str(event.get("timestamp")).as_deref());
        if let Some(timestamp) = timestamp {
            stats.start.get_or_insert(timestamp);
            stats.end = Some(timestamp);
        }

        let event_type = event.get("type").and_then(Value::as_str);
        let payload = event.get("payload").filter(|value| value.is_object());

        if event_type == Some("session_meta") {
            if let Some(payload) = payload {
                let meta_timestamp =
                    parse_timestamp(value_str(payload.get("timestamp")).as_deref());
                if stats.start.is_none() {
                    stats.start = meta_timestamp.or(timestamp);
                }
                assign_string(&mut stats.session_id, payload.get("id"));
                assign_string(&mut stats.cwd, payload.get("cwd"));
                assign_string(&mut stats.provider, payload.get("model_provider"));
                assign_string(&mut stats.cli_version, payload.get("cli_version"));
            }
            return;
        }

        if event_type == Some("turn_context") {
            if let Some(payload) = payload {
                assign_string(&mut stats.cwd, payload.get("cwd"));
                assign_string(&mut stats.model, payload.get("model"));
            }
            return;
        }

        if event_type != Some("event_msg") {
            return;
        }
        let Some(payload) = payload else {
            return;
        };
        if payload.get("type").and_then(Value::as_str) != Some("token_count") {
            return;
        }
        let Some(info) = payload.get("info").filter(|value| value.is_object()) else {
            return;
        };

        let total_usage = normalize_usage(info.get("total_token_usage"));
        let mut last_usage = normalize_usage(info.get("last_token_usage"));
        if let Some(context_window) = int_value(info.get("model_context_window")) {
            stats.context_window = Some(context_window);
        }

        let total_key = total_usage.key();
        if previous_total_key == Some(total_key) {
            stats.usage = total_usage;
            return;
        }

        if last_usage.total_tokens <= 0 {
            last_usage = total_usage.delta_from(previous_total_usage.as_ref());
        }

        stats.calls.push(CallStats {
            timestamp,
            usage: last_usage,
            running_total: total_usage.clone(),
            context_window: stats.context_window,
        });
        stats.usage = total_usage.clone();
        previous_total_key = Some(total_key);
        previous_total_usage = Some(total_usage);
    })?;

    Ok(stats)
}

pub fn parse_claude_file(path: &Path) -> UsageResult<SessionStats> {
    let mut stats = SessionStats::new(path.to_path_buf());
    stats.provider = Some("anthropic".to_string());
    let mut seen_messages = HashSet::new();
    let mut running = Usage::default();

    read_jsonl(path, |event, malformed| {
        if malformed {
            stats.malformed_lines += 1;
            return;
        }

        let timestamp = parse_timestamp(value_str(event.get("timestamp")).as_deref());
        if let Some(timestamp) = timestamp {
            stats.start.get_or_insert(timestamp);
            stats.end = Some(timestamp);
        }
        assign_string(&mut stats.session_id, event.get("sessionId"));
        assign_string(&mut stats.cwd, event.get("cwd"));
        assign_string(&mut stats.cli_version, event.get("version"));

        if event.get("type").and_then(Value::as_str) != Some("assistant") {
            return;
        }
        let Some(message) = event.get("message").filter(|value| value.is_object()) else {
            return;
        };

        let model = value_str(message.get("model"));
        if model.as_deref() == Some("<synthetic>") {
            return;
        }
        if let Some(model) = model {
            stats.model = Some(model);
        }

        if let Some(message_id) = value_key(message.get("id")) {
            if !seen_messages.insert(message_id) {
                return;
            }
        }

        let usage = claude_usage_to_fields(message.get("usage"));
        if usage.total_tokens <= 0 {
            return;
        }

        running = running.add(&usage);
        stats.calls.push(CallStats {
            timestamp,
            usage,
            running_total: running.clone(),
            context_window: None,
        });
        stats.usage = running.clone();
    })?;

    Ok(stats)
}

pub fn parse_pi_file(
    path: &Path,
    usage_mapper: fn(Option<&Value>) -> Usage,
) -> UsageResult<SessionStats> {
    let mut stats = SessionStats::new(path.to_path_buf());
    let mut seen_messages = HashSet::new();
    let mut running = Usage::default();
    let mut current_model = None;
    let mut current_provider = None;

    read_jsonl(path, |event, malformed| {
        if malformed {
            stats.malformed_lines += 1;
            return;
        }

        let timestamp = parse_timestamp(value_str(event.get("timestamp")).as_deref());
        if let Some(timestamp) = timestamp {
            stats.start.get_or_insert(timestamp);
            stats.end = Some(timestamp);
        }

        match event.get("type").and_then(Value::as_str) {
            Some("session") => {
                assign_string(&mut stats.session_id, event.get("id"));
                assign_string(&mut stats.cwd, event.get("cwd"));
            }
            Some("model_change") => {
                if let Some(provider) = value_str(event.get("provider")) {
                    current_provider = Some(provider);
                }
                if let Some(model) = value_str(event.get("modelId")) {
                    current_model = Some(model);
                }
                if current_provider.is_some() {
                    stats.provider = current_provider.clone();
                }
                if current_model.is_some() {
                    stats.model = current_model.clone();
                }
            }
            Some("message") => {
                let Some(message) = event.get("message").filter(|value| value.is_object()) else {
                    return;
                };
                if message.get("role").and_then(Value::as_str) != Some("assistant") {
                    return;
                }

                let message_key =
                    value_key(message.get("responseId")).or_else(|| value_key(event.get("id")));
                if let Some(message_key) = message_key {
                    if !seen_messages.insert(message_key) {
                        return;
                    }
                }

                stats.model = value_str(message.get("model")).or_else(|| current_model.clone());
                stats.provider =
                    value_str(message.get("provider")).or_else(|| current_provider.clone());

                let usage = usage_mapper(message.get("usage"));
                if usage.total_tokens <= 0 {
                    return;
                }

                running = running.add(&usage);
                stats.calls.push(CallStats {
                    timestamp,
                    usage,
                    running_total: running.clone(),
                    context_window: None,
                });
                stats.usage = running.clone();
            }
            _ => {}
        }
    })?;

    Ok(stats)
}

pub fn parse_openclaw_file(path: &Path) -> UsageResult<SessionStats> {
    parse_pi_file(path, openclaw_usage_to_fields)
}

pub fn parse_grok_dir(path: &Path) -> UsageResult<SessionStats> {
    let mut stats = SessionStats::new(path.to_path_buf());
    stats.provider = Some("xai".to_string());

    let (summary, summary_errors) = read_json_file(&path.join("summary.json"));
    let (signals, signal_errors) = read_json_file(&path.join("signals.json"));
    stats.malformed_lines += summary_errors + signal_errors;

    let info = summary.get("info").filter(|value| value.is_object());
    stats.session_id = info.and_then(|info| value_str(info.get("id"))).or_else(|| {
        path.file_name()
            .and_then(|name| name.to_str())
            .map(str::to_string)
    });
    stats.cwd = info.and_then(|info| value_str(info.get("cwd")));
    stats.start = parse_timestamp(value_str(summary.get("created_at")).as_deref());
    stats.end = parse_timestamp(value_str(summary.get("updated_at")).as_deref()).or(stats.start);

    stats.model = value_str(signals.get("primaryModelId")).or_else(|| {
        signals
            .get("modelsUsed")
            .and_then(Value::as_array)
            .and_then(|items| items.first())
            .and_then(Value::as_str)
            .map(str::to_string)
    });
    stats.context_window = int_value(signals.get("contextWindowTokens"));

    let total_tokens = int_value(signals.get("contextTokensUsed")).unwrap_or(0)
        + int_value(signals.get("totalTokensBeforeCompaction")).unwrap_or(0);
    let usage = Usage {
        input_tokens: total_tokens,
        total_tokens,
        ..Usage::default()
    };
    stats.usage = usage.clone();
    stats.call_count_override = Some(int_value(signals.get("turnCount")).unwrap_or(0) as usize);

    if total_tokens > 0 {
        stats.calls.push(CallStats {
            timestamp: stats.end,
            usage: usage.clone(),
            running_total: usage,
            context_window: stats.context_window,
        });
    }

    Ok(stats)
}

pub fn parse_opencode_db(path: &Path) -> UsageResult<Vec<SessionStats>> {
    let connection = Connection::open(path)?;
    let call_counts = read_opencode_call_counts(&connection);
    let mut statement = connection.prepare(
        r#"
        select
            id,
            directory,
            time_created,
            time_updated,
            model,
            tokens_input,
            tokens_output,
            tokens_reasoning,
            tokens_cache_read,
            tokens_cache_write
        from session
        "#,
    )?;
    let rows = statement.query_map([], |row| session_from_opencode_row(path, row, &call_counts))?;

    let mut sessions = Vec::new();
    for row in rows {
        sessions.push(row?);
    }
    Ok(sessions)
}

pub fn claude_usage_to_fields(raw: Option<&Value>) -> Usage {
    let new_input = int_value(raw.and_then(|raw| raw.get("input_tokens"))).unwrap_or(0);
    let cache_read = int_value(raw.and_then(|raw| raw.get("cache_read_input_tokens"))).unwrap_or(0);
    let cache_creation =
        int_value(raw.and_then(|raw| raw.get("cache_creation_input_tokens"))).unwrap_or(0);
    let output = int_value(raw.and_then(|raw| raw.get("output_tokens"))).unwrap_or(0);

    Usage {
        input_tokens: new_input + cache_creation + cache_read,
        cached_input_tokens: cache_read,
        output_tokens: output,
        reasoning_output_tokens: 0,
        total_tokens: new_input + cache_creation + cache_read + output,
    }
}

pub fn pi_usage_to_fields(raw: Option<&Value>) -> Usage {
    let new_input = int_value(raw.and_then(|raw| raw.get("input"))).unwrap_or(0);
    let cache_read = int_value(raw.and_then(|raw| raw.get("cacheRead"))).unwrap_or(0);
    let cache_write = int_value(raw.and_then(|raw| raw.get("cacheWrite"))).unwrap_or(0);
    let output = int_value(raw.and_then(|raw| raw.get("output"))).unwrap_or(0);
    let total = int_value(raw.and_then(|raw| raw.get("totalTokens"))).unwrap_or(0);

    Usage {
        input_tokens: new_input + cache_write + cache_read,
        cached_input_tokens: cache_read,
        output_tokens: output,
        reasoning_output_tokens: 0,
        total_tokens: if total > 0 {
            total
        } else {
            new_input + cache_write + cache_read + output
        },
    }
}

pub fn openclaw_usage_to_fields(raw: Option<&Value>) -> Usage {
    let mut usage = pi_usage_to_fields(raw);
    usage.total_tokens = usage.input_tokens + usage.output_tokens;
    usage
}

fn normalize_usage(raw: Option<&Value>) -> Usage {
    Usage {
        input_tokens: int_value(raw.and_then(|raw| raw.get("input_tokens"))).unwrap_or(0),
        cached_input_tokens: int_value(raw.and_then(|raw| raw.get("cached_input_tokens")))
            .unwrap_or(0),
        output_tokens: int_value(raw.and_then(|raw| raw.get("output_tokens"))).unwrap_or(0),
        reasoning_output_tokens: int_value(raw.and_then(|raw| raw.get("reasoning_output_tokens")))
            .unwrap_or(0),
        total_tokens: int_value(raw.and_then(|raw| raw.get("total_tokens"))).unwrap_or(0),
    }
}

fn read_jsonl<F>(path: &Path, mut on_event: F) -> UsageResult<()>
where
    F: FnMut(&Value, bool),
{
    let handle = File::open(path)?;
    for line in BufReader::new(handle).lines() {
        let line = line?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        match serde_json::from_str::<Value>(line) {
            Ok(event) => on_event(&event, false),
            Err(_) => on_event(&Value::Null, true),
        }
    }
    Ok(())
}

fn read_json_file(path: &Path) -> (Value, usize) {
    match std::fs::read_to_string(path) {
        Ok(text) => match serde_json::from_str::<Value>(&text) {
            Ok(value @ Value::Object(_)) => (value, 0),
            Ok(_) => (Value::Object(Default::default()), 0),
            Err(_) => (Value::Object(Default::default()), 1),
        },
        Err(_) => (Value::Object(Default::default()), 1),
    }
}

fn int_value(value: Option<&Value>) -> Option<i64> {
    match value? {
        Value::Number(number) => number
            .as_i64()
            .or_else(|| number.as_u64().map(|value| value as i64)),
        Value::String(text) => text.parse::<i64>().ok(),
        Value::Bool(true) => Some(1),
        Value::Bool(false) | Value::Null | Value::Array(_) | Value::Object(_) => Some(0),
    }
}

fn value_str(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(text) => Some(text.clone()),
        _ => None,
    }
}

fn value_key(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(value) => Some(value.to_string()),
        Value::Null | Value::Array(_) | Value::Object(_) => None,
    }
}

fn assign_string(target: &mut Option<String>, value: Option<&Value>) {
    if let Some(value) = value_str(value) {
        *target = Some(value);
    }
}

fn read_opencode_call_counts(connection: &Connection) -> HashMap<String, usize> {
    if let Ok(mut statement) = connection.prepare(
        r#"
        select session_id, count(*) as calls
        from message
        where json_extract(data, '$.role') = 'assistant'
        group by session_id
        "#,
    ) {
        if let Ok(rows) = statement.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as usize))
        }) {
            return rows.filter_map(Result::ok).collect();
        }
    }

    let mut counts = HashMap::new();
    let Ok(mut statement) = connection.prepare("select session_id, data from message") else {
        return counts;
    };
    let Ok(rows) = statement.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    }) else {
        return counts;
    };

    for (session_id, data) in rows.filter_map(Result::ok) {
        let Ok(Value::Object(data)) = serde_json::from_str::<Value>(&data) else {
            continue;
        };
        if data.get("role").and_then(Value::as_str) == Some("assistant") {
            *counts.entry(session_id).or_insert(0) += 1;
        }
    }
    counts
}

fn session_from_opencode_row(
    path: &Path,
    row: &Row<'_>,
    call_counts: &HashMap<String, usize>,
) -> rusqlite::Result<SessionStats> {
    let mut stats = SessionStats::new(path.to_path_buf());
    let session_id: String = row.get("id")?;
    stats.session_id = Some(session_id.clone());
    stats.cwd = row.get::<_, Option<String>>("directory")?;
    stats.start = parse_epoch_millis(row.get::<_, Option<i64>>("time_created")?);
    stats.end = parse_epoch_millis(row.get::<_, Option<i64>>("time_updated")?).or(stats.start);
    let model_raw: Option<String> = row.get("model")?;
    let (model, provider) = parse_opencode_model(model_raw.as_deref());
    stats.model = model;
    stats.provider = provider;
    stats.usage = opencode_usage_from_row(row)?;
    stats.call_count_override = Some(*call_counts.get(&session_id).unwrap_or(&0));

    if stats.usage.total_tokens > 0 {
        stats.calls.push(CallStats {
            timestamp: stats.end,
            usage: stats.usage.clone(),
            running_total: stats.usage.clone(),
            context_window: None,
        });
    }
    Ok(stats)
}

fn opencode_usage_from_row(row: &Row<'_>) -> rusqlite::Result<Usage> {
    let new_input: i64 = row.get("tokens_input")?;
    let cache_read: i64 = row.get("tokens_cache_read")?;
    let cache_write: i64 = row.get("tokens_cache_write")?;
    let output: i64 = row.get("tokens_output")?;
    let reasoning: i64 = row.get("tokens_reasoning")?;
    let input = new_input + cache_write + cache_read;

    Ok(Usage {
        input_tokens: input,
        cached_input_tokens: cache_read,
        output_tokens: output,
        reasoning_output_tokens: reasoning,
        total_tokens: input + output + reasoning,
    })
}

fn parse_opencode_model(raw: Option<&str>) -> (Option<String>, Option<String>) {
    let Some(raw) = raw.filter(|raw| !raw.is_empty()) else {
        return (None, None);
    };
    let Ok(Value::Object(data)) = serde_json::from_str::<Value>(raw) else {
        return (Some(raw.to_string()), None);
    };
    let model = data
        .get("id")
        .or_else(|| data.get("modelID"))
        .or_else(|| data.get("model"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let provider = data
        .get("providerID")
        .or_else(|| data.get("provider"))
        .and_then(Value::as_str)
        .map(str::to_string);
    (model, provider)
}

fn parse_epoch_millis(value: Option<i64>) -> Option<chrono::DateTime<Utc>> {
    Utc.timestamp_millis_opt(value?).single()
}
