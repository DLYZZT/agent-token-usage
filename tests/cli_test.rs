use agent_token_usage::cli::{Args, run};
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

fn args(paths: Vec<PathBuf>) -> Args {
    Args {
        paths,
        source: "codex".to_string(),
        latest: false,
        all: true,
        since: None,
        until: None,
        by_session: false,
        calls: false,
        json: true,
        csv: false,
        limit: 0,
        sort: "time".to_string(),
    }
}

fn codex_fixture(id: &str, date: &str, total: i64) -> String {
    let usage = json!({
        "input_tokens": total - 20,
        "cached_input_tokens": 0,
        "output_tokens": 20,
        "reasoning_output_tokens": 0,
        "total_tokens": total,
    });
    let meta = json!({
        "timestamp": format!("{date}T10:00:00Z"),
        "type": "session_meta",
        "payload": {"id": id, "cwd": "/tmp/p", "model_provider": "openai"},
    });
    let count = json!({
        "timestamp": format!("{date}T10:01:00Z"),
        "type": "event_msg",
        "payload": {"type": "token_count", "info": {"total_token_usage": usage.clone(), "last_token_usage": usage}},
    });
    format!("{meta}\n{count}\n")
}

fn run_json(args: Args) -> Value {
    serde_json::from_str(&run(args).unwrap()).unwrap()
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
                ('s1','/tmp/p',1750000000000,1750003600000,'gpt-5',100,50,0,0,0),
                ('s2','/tmp/q',1750100000000,1750100600000,'gpt-5',7,3,0,0,0);
            insert into message values
                ('m1','s1','{"role":"assistant"}'),
                ('m2','s2','{"role":"assistant"}');
            "#,
        )
        .unwrap();
}

#[test]
fn corrupt_file_is_skipped_instead_of_aborting() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("ok.jsonl"),
        codex_fixture("sess-1", "2026-06-30", 100),
    )
    .unwrap();
    // 无效的 SQLite 文件：解析必然失败，但不应中止整次统计。
    fs::write(
        dir.path().join("opencode.db"),
        b"this is not a sqlite database",
    )
    .unwrap();

    let payload = run_json(args(vec![dir.path().to_path_buf()]));
    assert_eq!(payload["summary"]["sessions"], 1);
    assert_eq!(payload["sessions"][0]["session_id"], "sess-1");
    assert_eq!(payload["summary"]["usage"]["total_tokens"], 100);
}

#[test]
fn all_reports_every_session() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("a.jsonl"),
        codex_fixture("sess-a", "2026-06-30", 100),
    )
    .unwrap();
    fs::write(
        dir.path().join("b.jsonl"),
        codex_fixture("sess-b", "2026-07-01", 200),
    )
    .unwrap();

    let payload = run_json(args(vec![dir.path().to_path_buf()]));
    assert_eq!(payload["summary"]["sessions"], 2);
    assert_eq!(payload["summary"]["usage"]["total_tokens"], 300);
}

#[test]
fn since_filters_out_older_sessions() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("a.jsonl"),
        codex_fixture("sess-a", "2026-06-30", 100),
    )
    .unwrap();
    fs::write(
        dir.path().join("b.jsonl"),
        codex_fixture("sess-b", "2026-07-01", 200),
    )
    .unwrap();

    let mut filtered = args(vec![dir.path().to_path_buf()]);
    filtered.since = Some("2026-07-01".to_string());
    let payload = run_json(filtered);
    assert_eq!(payload["summary"]["sessions"], 1);
    assert_eq!(payload["sessions"][0]["session_id"], "sess-b");
}

#[test]
fn latest_narrows_multi_session_db_to_one_session() {
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("opencode.db");
    create_opencode_db(&db_path);

    let mut latest = args(vec![db_path]);
    latest.all = false;
    let payload = run_json(latest);
    // opencode.db 含多会话，--latest 应在解析后收敛到 end 时间最新的一个。
    assert_eq!(payload["summary"]["sessions"], 1);
    assert_eq!(payload["sessions"][0]["session_id"], "s2");
}

#[test]
fn summary_text_output_smoke() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("a.jsonl"),
        codex_fixture("sess-a", "2026-06-30", 100),
    )
    .unwrap();

    let mut text_args = args(vec![dir.path().to_path_buf()]);
    text_args.json = false;
    let output = run(text_args).unwrap();
    assert!(output.contains("Codex token usage"), "output: {output}");
    assert!(output.contains("Total:    100"), "output: {output}");
}
