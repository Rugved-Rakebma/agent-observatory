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
- **Discovery poll (10s)** — scans sessions dir for new/dead PIDs
- **Enrichment (mtime-cached)** — `EnrichmentCache` stores parsed data per session, only re-parses JSONL when file mtime changes
- **Two-layer status**:
  - Layer 1: JSONL tail parsing (`enrichment.rs`) — reads last 32KB of `~/.claude/projects/{encoded_cwd}/{sessionId}.jsonl`, infers status from last message type + `stop_reason` + file mtime
  - Layer 2: Hook receiver (`hooks.rs`) — axum on `:7890`, receives POSTs from `~/.claude/hooks/features/observatory.sh`. Hook status takes priority, expires after 60s
- **Enriched data per session**: slug, model (Opus/Sonnet/Haiku), context window usage (input_tokens / model max), git branch, last assistant message snippet
- **Terminal detection**: `KERN_PROCARGS2` reads `TERM_PROGRAM` from Claude processes
- **Click-to-focus**: AppleScript for iTerm2 (tab-level via session UUID), app activation for others

## Project Structure

```
frontend/
  src/                    # Svelte 5 frontend
    routes/+page.svelte   # Main dashboard UI (3-row agent cards)
    app.css               # Tailwind theme + animations
  src-tauri/src/
    lib.rs                # Tauri setup, commands, poll timer, cache pruning
    scanner.rs            # Session discovery, PID validation, terminal detection
    enrichment.rs         # JSONL parsing, mtime cache, metadata extraction
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
- Context usage = `input_tokens + cache_creation_input_tokens + cache_read_input_tokens` from last assistant message's `usage` block (input_tokens alone is just the non-cached sliver)
- Context max: 1M for opus/sonnet, 200k for haiku
- 10MB line size guard on JSONL parsing (OOM protection)
- Graceful degradation: all enrichment fields are `Option<T>`, cards fall back to status-only if parsing fails
