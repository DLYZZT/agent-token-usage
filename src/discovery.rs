use std::collections::BTreeSet;
use std::env;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub type UsageResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

pub fn default_roots(source: &str) -> Vec<PathBuf> {
    match source {
        "claude" => vec![claude_root()],
        "pi" => vec![pi_root()],
        "grok" => vec![grok_root()],
        "opencode" => vec![opencode_root()],
        "openclaw" => vec![openclaw_root()],
        "copilot" => vec![copilot_root()],
        "all" => vec![
            codex_root(),
            claude_root(),
            pi_root(),
            grok_root(),
            opencode_root(),
            openclaw_root(),
            copilot_root(),
        ],
        _ => vec![codex_root()],
    }
}

pub fn codex_root() -> PathBuf {
    home_dir().join(".codex").join("sessions")
}

pub fn claude_root() -> PathBuf {
    home_dir().join(".claude").join("projects")
}

pub fn pi_root() -> PathBuf {
    home_dir().join(".pi").join("agent").join("sessions")
}

pub fn grok_root() -> PathBuf {
    home_dir().join(".grok")
}

pub fn opencode_root() -> PathBuf {
    home_dir().join(".local").join("share").join("opencode")
}

pub fn openclaw_root() -> PathBuf {
    home_dir()
        .join(".openclaw")
        .join("agents")
        .join("main")
        .join("sessions")
}

pub fn copilot_root() -> PathBuf {
    home_dir().join(".copilot").join("session-state")
}

pub fn is_opencode_db(path: &Path) -> bool {
    path.is_file() && path.file_name().and_then(|name| name.to_str()) == Some("opencode.db")
}

pub fn is_openclaw_trajectory_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.ends_with(".trajectory.jsonl"))
}

pub fn is_grok_session_dir(path: &Path) -> bool {
    path.join("summary.json").is_file() && path.join("signals.json").is_file()
}

pub fn find_grok_session_dirs(path: &Path) -> Vec<PathBuf> {
    if is_grok_session_dir(path) {
        return vec![path.to_path_buf()];
    }

    let mut dirs = BTreeSet::new();
    for entry in WalkDir::new(path).into_iter().filter_map(Result::ok) {
        if entry.file_type().is_file()
            && entry.file_name() == "signals.json"
            && entry.path().parent().is_some_and(is_grok_session_dir)
        {
            dirs.insert(entry.path().parent().unwrap().to_path_buf());
        }
    }
    dirs.into_iter().collect()
}

pub fn find_usage_paths(paths: &[PathBuf], allow_missing: bool) -> UsageResult<Vec<PathBuf>> {
    let mut files = BTreeSet::new();

    for path in paths {
        let expanded = expand_tilde(path);
        if expanded.is_file() {
            if !is_openclaw_trajectory_file(&expanded) {
                files.insert(expanded);
            }
        } else if expanded.is_dir() {
            let opencode_db = expanded.join("opencode.db");
            if opencode_db.is_file() {
                files.insert(opencode_db);
            }
            for session_dir in find_grok_session_dirs(&expanded) {
                files.insert(session_dir);
            }
            for entry in WalkDir::new(&expanded).into_iter().filter_map(Result::ok) {
                if entry.file_type().is_file()
                    && entry.path().extension().and_then(|ext| ext.to_str()) == Some("jsonl")
                    && !is_openclaw_trajectory_file(entry.path())
                {
                    files.insert(entry.path().to_path_buf());
                }
            }
        } else if !allow_missing {
            return Err(format!("Path not found: {}", path.display()).into());
        }
    }

    Ok(files.into_iter().collect())
}

fn expand_tilde(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    if text == "~" {
        return home_dir();
    }
    if let Some(rest) = text.strip_prefix("~/") {
        return home_dir().join(rest);
    }
    path.to_path_buf()
}

pub fn home_dir() -> PathBuf {
    env::var_os("HOME")
        .or_else(|| env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .or_else(|| {
            let drive = env::var_os("HOMEDRIVE")?;
            let path = env::var_os("HOMEPATH")?;
            let mut combined = drive;
            combined.push(path);
            Some(PathBuf::from(combined))
        })
        .unwrap_or_else(|| PathBuf::from("."))
}
