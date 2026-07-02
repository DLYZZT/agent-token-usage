use crate::discovery::{UsageResult, default_roots, find_usage_paths, is_opencode_db};
use crate::model::SessionStats;
use crate::parsers::parse_session_files;
use crate::render::{
    calls_table, render_calls_csv, render_json, render_sessions_csv, render_summary,
    render_summary_csv, sessions_table,
};
use crate::time::parse_filter_time;
use chrono::{DateTime, Utc};
use clap::Parser;
use rayon::prelude::*;
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "agent-token-usage",
    about = "统计 Agent 会话日志中的 token 消耗。默认只统计最近一次所有会话。"
)]
pub struct Args {
    #[arg(help = "会话 JSONL 文件、OpenCode 数据库或目录，默认按 --source 选择目录")]
    pub paths: Vec<PathBuf>,

    #[arg(
        long,
        value_parser = ["codex", "claude", "pi", "grok", "opencode", "openclaw", "copilot", "all"],
        default_value = "all",
        help = "日志来源：codex、claude、pi、grok、opencode、openclaw、copilot 或 all"
    )]
    pub source: String,

    #[arg(long, conflicts_with = "all", help = "只统计最新会话，默认行为")]
    pub latest: bool,

    #[arg(long, help = "统计所有匹配会话")]
    pub all: bool,

    #[arg(long, help = "只统计该时间之后的会话，如 2026-06-30")]
    pub since: Option<String>,

    #[arg(long, help = "只统计该时间之前的会话，如 2026-07-01")]
    pub until: Option<String>,

    #[arg(long, help = "按会话输出明细")]
    pub by_session: bool,

    #[arg(long, help = "按模型调用输出明细")]
    pub calls: bool,

    #[arg(long = "json", help = "输出 JSON")]
    pub json: bool,

    #[arg(long, help = "输出 CSV")]
    pub csv: bool,

    #[arg(long, default_value_t = 20, help = "明细输出行数，0 表示不限制")]
    pub limit: i64,

    #[arg(
        long,
        value_parser = ["time", "tokens"],
        default_value = "time",
        help = "明细排序方式"
    )]
    pub sort: String,
}

pub fn run_from_env() -> UsageResult<()> {
    let args = Args::parse();
    let output = run(args)?;
    print!("{output}");
    Ok(())
}

pub fn run(args: Args) -> UsageResult<String> {
    let roots = if args.paths.is_empty() {
        default_roots(&args.source)
    } else {
        args.paths.clone()
    };

    let since = parse_filter_time(args.since.as_deref(), false)
        .map_err(|error| format!("error: {error}"))?;
    let until = parse_filter_time(args.until.as_deref(), true)
        .map_err(|error| format!("error: {error}"))?;

    let mut files = find_usage_paths(&roots, args.paths.is_empty())?;
    if !args.all {
        files = latest_file(files);
    }

    let has_multi_session_file = files.iter().any(|path| is_opencode_db(path));
    let parsed: Vec<Vec<SessionStats>> = files
        .par_iter()
        .filter_map(|path| match parse_session_files(path) {
            Ok(sessions) => Some(sessions),
            Err(error) => {
                eprintln!("warning: skipped {}: {error}", path.display());
                None
            }
        })
        .collect();
    let mut sessions = parsed.into_iter().flatten().collect::<Vec<_>>();

    if !args.all && has_multi_session_file {
        sessions = latest_session(sessions);
    }

    sessions.retain(|session| in_time_range(session, since, until));
    sessions.retain(|session| session.call_count() > 0 || session.usage.total_tokens > 0);
    sort_sessions(&mut sessions, &args.sort);

    if args.json {
        return render_json(&sessions, args.calls);
    }

    if args.csv {
        if args.calls {
            return render_calls_csv(&sessions);
        }
        if args.by_session {
            return render_sessions_csv(&sessions);
        }
        return render_summary_csv(&sessions, if args.all { "all" } else { "latest" });
    }

    let visible_sessions = if args.limit > 0 && args.by_session {
        sessions
            .iter()
            .take(args.limit as usize)
            .cloned()
            .collect::<Vec<_>>()
    } else {
        sessions.clone()
    };

    if args.calls {
        return calls_table(&visible_sessions, &args.sort, args.limit);
    }
    if args.by_session {
        return sessions_table(&visible_sessions);
    }

    Ok(render_summary(
        &sessions,
        if args.all {
            "all sessions"
        } else {
            "latest session"
        },
        &args.source,
    ))
}

fn latest_file(files: Vec<PathBuf>) -> Vec<PathBuf> {
    files
        .into_iter()
        .max_by_key(|path| {
            path.metadata()
                .and_then(|metadata| metadata.modified())
                .ok()
        })
        .into_iter()
        .collect()
}

fn latest_session(sessions: Vec<SessionStats>) -> Vec<SessionStats> {
    sessions
        .into_iter()
        .max_by_key(|session| session.end.or(session.start))
        .into_iter()
        .collect()
}

fn in_time_range(
    session: &SessionStats,
    since: Option<DateTime<Utc>>,
    until: Option<DateTime<Utc>>,
) -> bool {
    let value = session.start.or(session.end);
    let Some(value) = value else {
        return since.is_none() && until.is_none();
    };
    if since.is_some_and(|since| value < since) {
        return false;
    }
    if until.is_some_and(|until| value >= until) {
        return false;
    }
    true
}

fn sort_sessions(sessions: &mut [SessionStats], sort_key: &str) {
    if sort_key == "tokens" {
        sessions.sort_by(|left, right| {
            right
                .usage
                .total_tokens
                .cmp(&left.usage.total_tokens)
                .then_with(|| left.start.cmp(&right.start))
        });
    } else {
        sessions.sort_by_key(|session| session.start);
    }
}
