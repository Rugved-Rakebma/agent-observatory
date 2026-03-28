# Agent Observatory

Tauri 2 desktop app for monitoring all active Claude Code sessions on macOS.

## Stack

- **Backend**: Rust (Tauri 2.10, axum, tokio)
- **Frontend**: Svelte 5 + Tailwind 4
- **Package manager**: Bun
- **Build**: `just dev` (dev mode), `just build` (release)

## Architecture

- **Stateless on launch** — no persistence. Every startup scans `~/.claude/sessions/*.json`
- **Grouped by git root** — `cwd` normalized via `git rev-parse --show-toplevel`
- **Two-layer status enrichment**:
  - Layer 1: JSONL tail parsing (`enrichment.rs`) — reads last 16KB of `~/.claude/projects/{encoded_cwd}/{sessionId}.jsonl`, infers status from last message type + `stop_reason` + file mtime
  - Layer 2: Hook receiver (`hooks.rs`) — axum on `:7890`, receives POSTs from `~/.claude/hooks/features/observatory.sh`. Hook status takes priority, expires after 60s
- **Terminal detection**: `KERN_PROCARGS2` reads `TERM_PROGRAM` from Claude processes
- **Click-to-focus**: AppleScript for iTerm2 (tab-level via session UUID), app activation for others

## Project Structure

```
frontend/
  src/                    # Svelte 5 frontend
    routes/+page.svelte   # Main dashboard UI
    app.css               # Tailwind theme + animations
  src-tauri/src/
    lib.rs                # Tauri setup, commands, poll timer
    scanner.rs            # Session discovery, PID validation, terminal detection
    enrichment.rs         # JSONL transcript inference
    hooks.rs              # axum hook receiver server
```

## Just Recipes

- `just dev` — run in dev mode (hot reload)
- `just build` — build release binary
- `just check` — cargo check backend
- `just frontend` — run frontend only (no Tauri shell)

## Key Patterns

- Path encoding for JSONL lookup: `cwd.replace('/', '-').replace(' ', '-')`
- `proc_pidpath` is unreliable on macOS — `is_claude_process` trusts session file existence as fallback
- Hook events: `SessionStart`, `Stop`, `PermissionRequest`, `UserPromptSubmit`, `Notification`, `PostToolUseFailure`
- Status inference from JSONL: `stop_reason: "end_turn"` = Idle, `"tool_use"` + stale mtime = WaitingInput
