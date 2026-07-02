use agent_token_usage::discovery::is_grok_session_dir;
use agent_token_usage::parsers::{detect_format, parse_session_file, parse_session_files};
use chrono::{TimeZone, Utc};
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

fn write_fixture(dir: &TempDir, name: &str, content: &str) -> PathBuf {
    let path = dir.path().join(name);
    fs::write(&path, content).unwrap();
    path
}

const CODEX_FIXTURE: &str = r#"
{"timestamp":"2026-06-30T10:00:00Z","type":"session_meta","payload":{"id":"sess-1","timestamp":"2026-06-30T10:00:00Z","cwd":"/tmp/project","model_provider":"openai","cli_version":"1.0.0"}}
{"timestamp":"2026-06-30T10:00:05Z","type":"turn_context","payload":{"cwd":"/tmp/project","model":"gpt-5-codex"}}
{"timestamp":"2026-06-30T10:00:10Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":100,"cached_input_tokens":20,"output_tokens":50,"reasoning_output_tokens":10,"total_tokens":160},"last_token_usage":{"input_tokens":100,"cached_input_tokens":20,"output_tokens":50,"reasoning_output_tokens":10,"total_tokens":160},"model_context_window":272000}}}
{"timestamp":"2026-06-30T10:01:00Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":300,"cached_input_tokens":120,"output_tokens":90,"reasoning_output_tokens":15,"total_tokens":405},"last_token_usage":{"input_tokens":0,"cached_input_tokens":0,"output_tokens":0,"reasoning_output_tokens":0,"total_tokens":0}}}}
{"timestamp":"2026-06-30T10:02:00Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":300,"cached_input_tokens":120,"output_tokens":90,"reasoning_output_tokens":15,"total_tokens":405},"last_token_usage":{"input_tokens":0,"cached_input_tokens":0,"output_tokens":0,"reasoning_output_tokens":0,"total_tokens":0}}}}
"#;

const CLAUDE_FIXTURE: &str = r#"
{"type":"user","timestamp":"2026-06-30T09:00:00Z","sessionId":"c-1","cwd":"/tmp/p","version":"2.0.0","message":{"role":"user","content":"hi"}}
{"type":"assistant","timestamp":"2026-06-30T09:00:05Z","sessionId":"c-1","message":{"id":"msg_1","model":"claude-opus-4-8","usage":{"input_tokens":10,"cache_creation_input_tokens":5,"cache_read_input_tokens":100,"output_tokens":20}}}
{"type":"assistant","timestamp":"2026-06-30T09:00:06Z","sessionId":"c-1","message":{"id":"msg_1","model":"claude-opus-4-8","usage":{"input_tokens":10,"cache_creation_input_tokens":5,"cache_read_input_tokens":100,"output_tokens":20}}}
{"type":"assistant","timestamp":"2026-06-30T09:00:07Z","sessionId":"c-1","message":{"id":"msg_2","model":"<synthetic>","usage":{"input_tokens":1,"output_tokens":1}}}
"#;

const PI_FIXTURE: &str = r#"
{"type":"session","version":3,"id":"pi-1","timestamp":"2026-06-30T08:00:00Z","cwd":"/tmp/p"}
{"type":"model_change","timestamp":"2026-06-30T08:00:01Z","provider":"anthropic","modelId":"claude-sonnet-5"}
{"type":"message","timestamp":"2026-06-30T08:00:05Z","id":"m1","message":{"role":"assistant","responseId":"r1","usage":{"input":50,"cacheRead":10,"cacheWrite":5,"output":25,"totalTokens":90}}}
{"type":"message","timestamp":"2026-06-30T08:00:06Z","id":"m2","message":{"role":"assistant","responseId":"r1","usage":{"input":50,"cacheRead":10,"cacheWrite":5,"output":25,"totalTokens":90}}}
{"type":"message","timestamp":"2026-06-30T08:00:07Z","id":"m3","message":{"role":"user"}}
"#;

const OPENCLAW_FIXTURE: &str = r#"
{"type":"message","timestamp":"2026-06-30T07:00:00Z","id":"m1","message":{"role":"assistant","api":"anthropic","provider":"anthropic","model":"claude-opus-4-8","usage":{"input":100,"cacheRead":40,"cacheWrite":0,"output":30,"totalTokens":9999}}}
"#;

#[test]
fn detects_each_jsonl_format() {
    let dir = TempDir::new().unwrap();
    let cases = [
        ("codex.jsonl", CODEX_FIXTURE, "codex"),
        ("claude.jsonl", CLAUDE_FIXTURE, "claude"),
        ("pi.jsonl", PI_FIXTURE, "pi"),
        ("openclaw.jsonl", OPENCLAW_FIXTURE, "openclaw"),
    ];
    for (name, fixture, expected) in cases {
        let path = write_fixture(&dir, name, fixture);
        assert_eq!(detect_format(&path).unwrap(), expected, "fixture {name}");
    }
}

#[test]
fn detect_format_falls_back_to_codex() {
    let dir = TempDir::new().unwrap();
    let path = write_fixture(&dir, "unknown.jsonl", "{\"foo\":1}\nnot json\n");
    assert_eq!(detect_format(&path).unwrap(), "codex");
}

#[test]
fn detect_format_uses_pi_session_header_as_candidate() {
    let dir = TempDir::new().unwrap();
    let path = write_fixture(
        &dir,
        "header-only.jsonl",
        "{\"type\":\"session\",\"version\":3,\"id\":\"pi-2\"}\n",
    );
    assert_eq!(detect_format(&path).unwrap(), "pi");
}

#[test]
fn parses_codex_deltas_and_skips_duplicate_totals() {
    let dir = TempDir::new().unwrap();
    let path = write_fixture(&dir, "codex.jsonl", CODEX_FIXTURE);
    let stats = parse_session_file(&path).unwrap();

    assert_eq!(stats.session_id.as_deref(), Some("sess-1"));
    assert_eq!(stats.cwd.as_deref(), Some("/tmp/project"));
    assert_eq!(stats.provider.as_deref(), Some("openai"));
    assert_eq!(stats.model.as_deref(), Some("gpt-5-codex"));
    assert_eq!(stats.cli_version.as_deref(), Some("1.0.0"));
    assert_eq!(stats.context_window, Some(272000));
    assert_eq!(
        stats.start,
        Some(Utc.with_ymd_and_hms(2026, 6, 30, 10, 0, 0).unwrap())
    );
    assert_eq!(
        stats.end,
        Some(Utc.with_ymd_and_hms(2026, 6, 30, 10, 2, 0).unwrap())
    );

    // 第三个 token_count 与第二个 totals 相同，应被去重：只有 2 次调用。
    assert_eq!(stats.calls.len(), 2);
    assert_eq!(stats.usage.total_tokens, 405);

    // 第一次调用直接使用 last_token_usage。
    assert_eq!(stats.calls[0].usage.total_tokens, 160);
    // 第二次调用 last_token_usage 为 0，应从运行总量推导 delta。
    let delta = &stats.calls[1].usage;
    assert_eq!(delta.input_tokens, 200);
    assert_eq!(delta.cached_input_tokens, 100);
    assert_eq!(delta.output_tokens, 40);
    assert_eq!(delta.reasoning_output_tokens, 5);
    assert_eq!(delta.total_tokens, 245);
}

#[test]
fn parses_claude_usage_and_dedupes_message_ids() {
    let dir = TempDir::new().unwrap();
    let path = write_fixture(&dir, "claude.jsonl", CLAUDE_FIXTURE);
    let stats = parse_session_file(&path).unwrap();

    assert_eq!(stats.session_id.as_deref(), Some("c-1"));
    assert_eq!(stats.provider.as_deref(), Some("anthropic"));
    assert_eq!(stats.model.as_deref(), Some("claude-opus-4-8"));
    assert_eq!(stats.cli_version.as_deref(), Some("2.0.0"));

    // msg_1 重复出现一次、<synthetic> 应被跳过：只剩 1 次调用。
    assert_eq!(stats.calls.len(), 1);
    // input = input + cache_creation + cache_read
    assert_eq!(stats.usage.input_tokens, 115);
    assert_eq!(stats.usage.cached_input_tokens, 100);
    assert_eq!(stats.usage.output_tokens, 20);
    assert_eq!(stats.usage.total_tokens, 135);
}

#[test]
fn parses_pi_usage_and_dedupes_response_ids() {
    let dir = TempDir::new().unwrap();
    let path = write_fixture(&dir, "pi.jsonl", PI_FIXTURE);
    let stats = parse_session_file(&path).unwrap();

    assert_eq!(stats.session_id.as_deref(), Some("pi-1"));
    assert_eq!(stats.provider.as_deref(), Some("anthropic"));
    // message 没有 model 字段时回退到 model_change 记录的模型。
    assert_eq!(stats.model.as_deref(), Some("claude-sonnet-5"));

    // responseId r1 重复、user 消息不计：只剩 1 次调用。
    assert_eq!(stats.calls.len(), 1);
    // input = input + cacheWrite + cacheRead
    assert_eq!(stats.usage.input_tokens, 65);
    assert_eq!(stats.usage.cached_input_tokens, 10);
    assert_eq!(stats.usage.output_tokens, 25);
    // 有 totalTokens 时直接信任。
    assert_eq!(stats.usage.total_tokens, 90);
}

#[test]
fn openclaw_recomputes_total_tokens() {
    let dir = TempDir::new().unwrap();
    let path = write_fixture(&dir, "openclaw.jsonl", OPENCLAW_FIXTURE);
    let stats = parse_session_file(&path).unwrap();

    assert_eq!(stats.calls.len(), 1);
    assert_eq!(stats.usage.input_tokens, 140);
    assert_eq!(stats.usage.cached_input_tokens, 40);
    assert_eq!(stats.usage.output_tokens, 30);
    // openclaw 忽略来源的 totalTokens(9999)，重算为 input + output。
    assert_eq!(stats.usage.total_tokens, 170);
}

#[test]
fn tolerates_malformed_and_non_utf8_lines() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("dirty.jsonl");
    let mut content = Vec::new();
    content.extend_from_slice(CODEX_FIXTURE.trim_start().as_bytes());
    content.extend_from_slice(b"not json at all\n");
    content.extend_from_slice(b"{\"type\": \xff\xfe broken bytes}\n");
    fs::write(&path, content).unwrap();

    let stats = parse_session_file(&path).unwrap();
    assert_eq!(stats.malformed_lines, 2);
    // 坏行不影响正常事件的统计。
    assert_eq!(stats.calls.len(), 2);
    assert_eq!(stats.usage.total_tokens, 405);
}

fn write_grok_session(dir: &Path) {
    fs::write(
        dir.join("summary.json"),
        r#"{"info":{"id":"g-1","cwd":"/tmp/p"},"created_at":"2026-06-30T06:00:00Z","updated_at":"2026-06-30T06:30:00Z"}"#,
    )
    .unwrap();
    fs::write(
        dir.join("signals.json"),
        r#"{"primaryModelId":"grok-4","contextWindowTokens":256000,"contextTokensUsed":1000,"totalTokensBeforeCompaction":500,"turnCount":7}"#,
    )
    .unwrap();
}

#[test]
fn parses_grok_session_dir() {
    let dir = TempDir::new().unwrap();
    write_grok_session(dir.path());

    assert!(is_grok_session_dir(dir.path()));
    assert_eq!(detect_format(dir.path()).unwrap(), "grok");

    let stats = parse_session_file(dir.path()).unwrap();
    assert_eq!(stats.session_id.as_deref(), Some("g-1"));
    assert_eq!(stats.provider.as_deref(), Some("xai"));
    assert_eq!(stats.model.as_deref(), Some("grok-4"));
    assert_eq!(stats.context_window, Some(256000));
    // contextTokensUsed + totalTokensBeforeCompaction
    assert_eq!(stats.usage.total_tokens, 1500);
    assert_eq!(stats.usage.input_tokens, 1500);
    // 调用数来自 turnCount，明细只有 1 条聚合记录。
    assert_eq!(stats.call_count(), 7);
    assert_eq!(stats.calls.len(), 1);
}

fn create_opencode_db(path: &Path) {
    let connection = rusqlite::Connection::open(path).unwrap();
    connection
        .execute_batch(
            r#"
            create table session (
                id text primary key,
                directory text,
                time_created integer,
                time_updated integer,
                model text,
                tokens_input integer not null default 0,
                tokens_output integer not null default 0,
                tokens_reasoning integer not null default 0,
                tokens_cache_read integer not null default 0,
                tokens_cache_write integer not null default 0
            );
            create table message (id text, session_id text, data text);
            insert into session values
                ('s1','/tmp/p',1750000000000,1750003600000,'{"providerID":"anthropic","modelID":"claude-sonnet-5"}',100,50,10,30,20),
                ('s2','/tmp/q',1750100000000,1750100600000,'gpt-5',7,3,0,0,0);
            insert into message values
                ('m1','s1','{"role":"assistant"}'),
                ('m2','s1','{"role":"user"}'),
                ('m3','s1','{"role":"assistant"}'),
                ('m4','s2','{"role":"assistant"}');
            "#,
        )
        .unwrap();
}

#[test]
fn parses_opencode_db_sessions() {
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("opencode.db");
    create_opencode_db(&db_path);

    let mut sessions = parse_session_files(&db_path).unwrap();
    sessions.sort_by(|a, b| a.session_id.cmp(&b.session_id));
    assert_eq!(sessions.len(), 2);

    let s1 = &sessions[0];
    assert_eq!(s1.session_id.as_deref(), Some("s1"));
    assert_eq!(s1.cwd.as_deref(), Some("/tmp/p"));
    assert_eq!(s1.model.as_deref(), Some("claude-sonnet-5"));
    assert_eq!(s1.provider.as_deref(), Some("anthropic"));
    // input = input + cache_write + cache_read
    assert_eq!(s1.usage.input_tokens, 150);
    assert_eq!(s1.usage.cached_input_tokens, 30);
    assert_eq!(s1.usage.output_tokens, 50);
    assert_eq!(s1.usage.reasoning_output_tokens, 10);
    assert_eq!(s1.usage.total_tokens, 210);
    // 调用数来自 message 表中 assistant 消息计数。
    assert_eq!(s1.call_count(), 2);

    let s2 = &sessions[1];
    // model 列不是 JSON 时按原文使用。
    assert_eq!(s2.model.as_deref(), Some("gpt-5"));
    assert_eq!(s2.provider, None);
    assert_eq!(s2.usage.total_tokens, 10);
    assert_eq!(s2.call_count(), 1);
}
