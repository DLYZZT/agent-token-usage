# agent-token-usage

[English](README.md) | [简体中文](README.zh-CN.md)

A small Rust CLI that reads the local session logs written by coding-agent CLIs (Codex, Claude Code, Pi, Grok, opencode, openclaw) and reports how many tokens they used — no network calls, no API keys, everything is computed from files already on your disk.

## Features

- **Multi-source**: understands the on-disk session formats of Codex, Claude Code, Pi, Grok, opencode, and openclaw, plus an `all` mode that scans every known source at once.
- **Multiple scopes**: defaults to your most recent Codex session; use `--all` to aggregate every session found, or `--since`/`--until` to filter by time range.
- **Multiple output shapes**: a one-shot summary, a per-session table, or a per-model-call table — each also available as JSON or CSV for scripting.
- **Fast**: session files are parsed in parallel with [rayon](https://crates.io/crates/rayon).

## Installation

Requires a recent Rust toolchain (edition 2024).

```bash
git clone <this-repo>
cd agent-token-usage
cargo build --release
# binary at target/release/agent-token-usage
```

Or run directly during development:

```bash
cargo run -- [args]
```

## Quick start

```bash
# Token usage for your most recent Codex session (default)
agent-token-usage

# Same, but for Claude Code
agent-token-usage --source claude

# All sessions across every supported source
agent-token-usage --source all --all

# Only sessions since a given date
agent-token-usage --source claude --all --since 2026-06-01
```

Example output:

```
$ agent-token-usage --source claude
Claude token usage (latest session)
Sessions: 1    Calls: 9
Period:   2026-07-01 06:53:24 -> 2026-07-01 06:58:43
Input:    433,260
Cached:   384,823
Uncached: 48,437
Output:   4,282
Reason:   0
Total:    437,542
```

## Supported sources

`--source` selects which agent's logs to scan and picks the corresponding default directory when no explicit path is given:

| `--source`  | Default root                          | On-disk format |
|-------------|----------------------------------------|----------------|
| `codex`     | `~/.codex/sessions`                    | JSONL (default) |
| `claude`    | `~/.claude/projects`                   | JSONL |
| `pi`        | `~/.pi/agent/sessions`                 | JSONL |
| `grok`      | `~/.grok`                              | directories containing `summary.json` + `signals.json` |
| `opencode`  | `~/.local/share/opencode`              | SQLite (`opencode.db`) |
| `openclaw`  | `~/.openclaw/agents/main/sessions`     | JSONL |
| `all`       | all of the above                       | — |

You can also pass one or more explicit files/directories instead of relying on the default root:

```bash
agent-token-usage ~/.codex/sessions/2026/07/some-session.jsonl
agent-token-usage --source opencode ~/some/other/opencode.db
```

## Output modes

- Default: an aggregate summary (sessions, calls, and token breakdown for the current scope).
- `--by-session`: one row per session.
- `--calls`: one row per model call (a request/response turn), across all matched sessions.
- `--json`: emit the summary (and sessions/calls, depending on the above flags) as JSON instead of a text table.
- `--csv`: emit the same data as CSV.

`--json` and `--csv` respect `--by-session`/`--calls` the same way the text output does.

## CLI reference

```
Usage: agent-token-usage [OPTIONS] [PATHS]...

Arguments:
  [PATHS]...  Session JSONL files, an OpenCode database, or directories to scan.
              Defaults to the --source's default directory when omitted.

Options:
      --source <SOURCE>  Log source: codex, claude, pi, grok, opencode, openclaw, or all
                          [default: codex]
      --latest           Only report the most recent session (default behavior)
      --all              Report every matching session instead of just the latest
      --since <SINCE>    Only include sessions on/after this time, e.g. 2026-06-30
      --until <UNTIL>    Only include sessions before this time, e.g. 2026-07-01
      --by-session       Print one row per session
      --calls            Print one row per model call
      --json             Output JSON
      --csv              Output CSV
      --limit <LIMIT>    Number of detail rows to print, 0 means unlimited [default: 20]
      --sort <SORT>      Sort detail rows by: time or tokens [default: time]
```

Notes:

- `--latest` and `--all` are mutually exclusive; `--latest` is simply the default and rarely needs to be passed explicitly.
- `--since`/`--until` accept a bare date (`2026-06-30`) or a full timestamp; a bare `--until` date is treated as exclusive of that whole day.
- `--limit` only affects table/detail output, not `--json`/`--csv`, and not the aggregate summary.
- For `opencode`, a single `opencode.db` file can hold many sessions; without `--all`, only the most recently updated session in that database is reported.

## Token fields

Every usage breakdown reports:

- `input_tokens` — total input tokens, including any cached/cache-creation tokens
- `cached_input_tokens` — the subset of input tokens served from cache
- `uncached_input_tokens` — `input_tokens - cached_input_tokens` (derived, never negative)
- `output_tokens`
- `reasoning_output_tokens` — reasoning/thinking tokens, when the source reports them separately
- `total_tokens`

Different agent tools report usage slightly differently (e.g. Codex reports a running total per turn and calls are derived as deltas, while Claude/Pi/openclaw report per-call usage that's accumulated). The CLI normalizes all of them into the fields above.

## Development

```bash
cargo build          # build
cargo run -- [args]  # run
cargo test           # run tests
cargo clippy         # lint
cargo fmt            # format
```