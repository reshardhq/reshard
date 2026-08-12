# Agent bridge — ACP

> Status: **built and working** (2026-08-08). The bridge that runs a real agent
> behind a chat, via the Agent Client Protocol (ACP). Supersedes the earlier
> "shell `claude -p` and parse the final blob" idea.

## Why ACP

The bridge must be **agent-agnostic** (pillar 3) — any coding agent, one code
path. Instead of a bespoke parser + permission hack per provider, we speak
**ACP** (Zed's Agent Client Protocol): one client drives Claude, Codex, Gemini,
Cursor, Cline, … over JSON-RPC/stdio, getting streaming tool events and
permission requests for free.

- We depend on the **official Rust SDK**: `agent-client-protocol = "2.0.0"`
  (repo `agentclientprotocol/rust-sdk`). Buzz (block/buzz) hand-rolls its own
  ACP wire; we don't — less code, less drift.
- ACP is coding-agent-flavoured. Non-ACP agents (Hermes, arbitrary CLIs) fall
  back to the old shell-exec path in `cli/src/up.rs`.

## The pieces

```
Tauri app ⇄ relay ⇄ gateway (up.rs) ⇄ reshard acp (ACP client) ⇄ claude-code-acp (ACP agent) ⇄ claude -p
```

- **`cli/src/acp.rs`** — the ACP **client**. Spawns an ACP agent over stdio,
  runs one prompt, translates `session/update` → text + tool-call telemetry and
  `session/request_permission` → a decision. Two modes:
  - `Terminal` — the interactive `reshard acp` probe (prints to the terminal).
  - `Gateway` — driven by the gateway: posts a `Cmd::Status` per tool call to
    the relay (live chips in the app) and prints the final reply to stdout for
    the gateway to `Cmd::Send`.
  - Also the **adapter resolver** (`adapter_command`): maps `--provider` →
    a launcher (see below).
- **`cli/src/bin/claude-code-acp.rs`** — our own ACP **agent** for Claude. Wraps
  `claude -p --output-format stream-json`. **No Node, no API key** — it strips
  `ANTHROPIC_API_KEY`/`ANTHROPIC_AUTH_TOKEN` so `claude` authenticates via the
  **Pro/Max subscription login**. Streams text + tool calls as ACP session
  updates; persists Claude's `session_id` per chat for continuity. Design ported
  from `harukitosa/claude-code-acp` (MIT).

## Provider resolution (`reshard acp --provider …`)

| provider | resolves to | auth |
|---|---|---|
| `claude` (default) | our `claude-code-acp` binary (co-located next to `reshard`) | **subscription** login, no Node |
| `claude-api` | `npx -y @agentclientprotocol/claude-agent-acp` (Agent SDK) | **API key** only (Anthropic blocks subscription for the SDK) |
| `codex` | `npx -y @zed-industries/codex-acp` | codex CLI login |
| `gemini` | `gemini --experimental-acp` (native) | gemini CLI login |
| `--command "…"` | verbatim override | — |

**Key auth fact:** the official `claude-agent-acp` adapter wraps the Claude
**Agent SDK**, which requires an API key (Anthropic policy blocks subscription
auth for the SDK). The only way to use the **subscription** is to wrap the
**CLI** (`claude -p`) — which is what `claude-code-acp` does. Buzz's Claude path
is API-key-only; ours isn't.

## How it plugs into the gateway (no `up.rs` surgery)

The gateway already runs a shell `exec` per message, captures stdout as the
reply (→ `Cmd::Send`), and sets `RESHARD_CONTEXT`/`RESHARD_CHAT`/`RESHARD_RELAY`/
`RESHARD_AS` env. So `reshard connect --provider claude` records:

```
exec = "reshard acp --provider claude --gateway --cwd '<project dir>'"
```

`reshard acp --gateway` reads the prompt from `$RESHARD_CONTEXT`, the machine
token from `~/.reshard/machine-token`, posts tool `Status` events to the relay,
and prints the reply on stdout for the gateway to `Send`. Tool chips + replies
appear live in the app.

## Working directory / project binding

`reshard connect --cwd <dir>` (default: the directory connect is run from) →
baked into the exec → passed as ACP `NewSessionRequest.cwd` → `claude -p` runs
**inside the project**, reads its `CLAUDE.md`/docs, like `cd project && claude`.
Buzz has **no** per-agent working dir (agents bind to channels only) — this is
our differentiator. Run `reshard connect` from the project dir, or pass `--cwd`.

## Session continuity (the "it remembers now" fix)

The gateway spawns the adapter **fresh per message**, so in-memory state can't
carry a conversation. Fix: `claude-code-acp` persists Claude's `session_id` per
chat at `~/.reshard/sessions/<chat>.txt` (keyed by `$RESHARD_CHAT`) and
`--resume`s it on the next message. Verified: two separate processes, the second
remembers the first. Continuity kicks in from the **2nd message** in a chat
onward (the first creates the session).

> Buzz keeps a **long-lived process** with a `channel_id → session_id` map and
> reuses the session in-memory (no disk). That's the more scalable model (avoids
> respawn/model-reinit cost) and is the upgrade path if per-message spawn latency
> hurts. Our disk-persist approach is simpler and, unlike Buzz, **survives a
> process crash**.

## Permission / tools

Headless `claude -p` has no one to approve tools, so a gated tool dead-ends
("please approve"). Reshard now keeps Claude in `default` permission mode and
fails closed. It does not add `--dangerously-skip-permissions`.

- Buzz **refuses** an unattended full-bypass (there's a test enforcing it). They
  use ACP permission **modes** (`default`/`acceptEdits`/`dontAsk`/`plan`) + the
  agent's own sandbox. But `acceptEdits` still gates Bash → wouldn't run a shell
  command headless, which is why Buzz's default runtime is Goose, not Claude.
- **The approval vertical slice is implemented:** native ACP permission calls
  pause in the local broker, route an owner-only card, and resume only one
  exact matching call after **Allow once**. Claude's print-mode path uses the
  same private `--permission-prompt-tool` MCP bridge.
  A **lent** agent's request routes to its owner's session, not to everyone in
  the room. This is the gate Buzz's workflow approval never enforced.

`--strict-mcp-config` is also passed to the adapter, so the agent does **not**
inherit the user's personal MCP servers (reminders/calendar/contacts) — those
trigger macOS TCC prompts attributed to `reshard`.

## Gotchas discovered

- **Apple Silicon code-signing.** `cp`-ing a new binary *over* an existing one
  invalidates its ad-hoc signature → macOS SIGKILLs it on launch (`zsh: killed`,
  exit 137). After any manual reinstall: `codesign --force --sign - <binary>`.
  The real `install.sh`/`cargo install` path builds fresh binaries and avoids
  this.
- **macOS TCC prompts** ("reshard wants your reminders") come from the user's
  Claude MCP servers, not reshard — fixed with `--strict-mcp-config`.
- **Seed collision:** the relay seeds a demo agent named `claude-main`; connect
  with a different name to avoid `resolve_member` ambiguity in a fresh DB.

## Open items / next

1. **Long-lived adapter + session reuse** (Buzz's pool model) if spawn latency
   or model re-init becomes a bottleneck.
2. **Persona file** (`.persona.md`: frontmatter + markdown-as-system-prompt,
   `runtime`/`model`/`triggers`) instead of piling flags onto `connect`.
3. **Codex/Gemini** adapters via the resolver (already wired; untested).

## CLI reference

```
reshard acp --provider claude -m "hi"                 # interactive probe
reshard acp --provider claude --dry-run -m x          # print resolved adapter, don't run
reshard acp --provider claude --gateway --cwd <dir>   # gateway mode (what connect records)
reshard connect <invite> --provider claude --name <n> [--cwd <dir>] [--no-gateway]
```
