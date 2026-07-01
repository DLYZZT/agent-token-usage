# agent-token-usage

[English](README.md) | [简体中文](README.zh-CN.md)

一个用 Rust 编写的小工具，用于读取各类 Agent CLI（Codex、Claude Code、Pi、Grok、opencode、openclaw）在本地磁盘上写的会话日志，统计它们消耗的 token 数量。全程不发起任何网络请求、不需要任何 API Key，所有数据都来自你本机已有的日志文件。

## 特性

- **多来源支持**：理解 Codex、Claude Code、Pi、Grok、opencode、openclaw 各自的本地会话格式，也支持 `all` 模式一次性扫描所有已知来源。
- **多种统计范围**：默认只统计最近一次 Codex 会话；可用 `--all` 统计所有找到的会话，或用 `--since`/`--until` 按时间范围过滤。
- **多种输出形式**：支持一次性汇总、按会话明细、按模型调用明细三种粒度，且都可以选择输出为 JSON 或 CSV 以便脚本处理。
- **速度快**：使用 [rayon](https://crates.io/crates/rayon) 并行解析会话文件。

## 安装

需要较新版本的 Rust 工具链（edition 2024）。

```bash
git clone https://github.com/DLYZZT/agent-token-usage
cd agent-token-usage
cargo build --release
# 生成的可执行文件位于 target/release/agent-token-usage
```

开发期间也可以直接运行：

```bash
cargo run -- [参数]
```

## 快速开始

```bash
# 统计最近一次 Codex 会话（默认行为）
agent-token-usage

# 统计 Claude Code 的会话
agent-token-usage --source claude

# 统计所有支持来源的所有会话
agent-token-usage --source all --all

# 只统计某个日期之后的会话
agent-token-usage --source claude --all --since 2026-06-01
```

示例输出：

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

## 支持的来源

`--source` 决定扫描哪个 Agent 工具的日志，并在未显式指定路径时选用对应的默认目录：

| `--source`  | 默认目录                               | 磁盘格式 |
|-------------|-----------------------------------------|----------|
| `codex`     | `~/.codex/sessions`                     | JSONL（默认值） |
| `claude`    | `~/.claude/projects`                    | JSONL |
| `pi`        | `~/.pi/agent/sessions`                  | JSONL |
| `grok`      | `~/.grok`                               | 同时包含 `summary.json` 和 `signals.json` 的目录 |
| `opencode`  | `~/.local/share/opencode`               | SQLite（`opencode.db`） |
| `openclaw`  | `~/.openclaw/agents/main/sessions`      | JSONL |
| `all`       | 以上全部                                | — |

也可以不使用默认目录，直接传入一个或多个具体的文件/目录：

```bash
agent-token-usage ~/.codex/sessions/2026/07/some-session.jsonl
agent-token-usage --source opencode ~/some/other/opencode.db
```

## 输出模式

- 默认：输出当前统计范围内的汇总信息（会话数、调用数、token 明细）。
- `--by-session`：按会话输出一行。
- `--calls`：按模型调用（一次请求/响应）输出一行，覆盖所有匹配到的会话。
- `--json`：以 JSON 而非文本表格输出（内容会根据上面两个 flag 相应调整）。
- `--csv`：以 CSV 格式输出同样的数据。

`--json` 与 `--csv` 会像文本输出一样遵循 `--by-session`/`--calls` 的选择。

## 命令行参数说明

```
Usage: agent-token-usage [OPTIONS] [PATHS]...

Arguments:
  [PATHS]...  会话 JSONL 文件、OpenCode 数据库或要扫描的目录。
              省略时使用 --source 对应的默认目录。

Options:
      --source <SOURCE>  日志来源：codex、claude、pi、grok、opencode、openclaw 或 all
                          [默认值: codex]
      --latest           只统计最新会话（默认行为）
      --all              统计所有匹配到的会话，而不仅是最新的
      --since <SINCE>    只统计该时间点（含）之后的会话，如 2026-06-30
      --until <UNTIL>    只统计该时间点之前的会话，如 2026-07-01
      --by-session       按会话输出明细
      --calls            按模型调用输出明细
      --json             输出 JSON
      --csv              输出 CSV
      --limit <LIMIT>    明细输出的行数，0 表示不限制 [默认值: 20]
      --sort <SORT>      明细排序方式：time 或 tokens [默认值: time]
```

补充说明：

- `--latest` 与 `--all` 互斥；`--latest` 本身就是默认行为，通常不需要显式传入。
- `--since`/`--until` 既可以传纯日期（如 `2026-06-30`），也可以传完整时间戳；`--until` 传纯日期时会视为不包含当天，即整天都会被过滤掉。
- `--limit` 只影响表格/明细输出，不影响 `--json`/`--csv` 的结果，也不影响汇总统计。
- 对于 `opencode` 来源，单个 `opencode.db` 文件里可能包含多个会话；未加 `--all` 时，只会报告该数据库中最近更新的一个会话。

## Token 字段说明

每次统计都会给出以下字段：

- `input_tokens` —— 输入 token 总数，包含所有缓存命中/缓存创建的 token
- `cached_input_tokens` —— 输入 token 中命中缓存的部分
- `uncached_input_tokens` —— `input_tokens - cached_input_tokens`（计算得出，不会为负）
- `output_tokens`
- `reasoning_output_tokens` —— 推理/思考 token，仅在数据来源单独上报时才有值
- `total_tokens`

不同 Agent 工具上报 usage 的方式略有差异（例如 Codex 每个回合上报一个累计总量，调用级别的用量是通过差值推算出来的；而 Claude/Pi/openclaw 则是每次调用单独上报，再由本工具累加）。本工具会将它们统一归一化为上述字段。

## macOS："已损坏，无法打开" / 运行后文件消失

提示"*agent-token-usage* 已损坏，无法打开"且只给"移到废纸篓"选项。这并不是文件真的损坏，下载后执行一次以下命令清除隔离标记即可：

```bash
xattr -dr com.apple.quarantine ./agent-token-usage
chmod +x ./agent-token-usage
```

## 开发

```bash
cargo build          # 构建
cargo run -- [参数]   # 运行
cargo test           # 运行测试
cargo clippy         # 代码检查
cargo fmt            # 格式化
```