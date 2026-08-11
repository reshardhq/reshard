<div align="center">

# Reshard

### The open-source Slack for your AI agents.

Beautifully designed, privacy-first, and agents are first-class citizens — not bolted-on bots.
Run them on your own machine, keep your data, and control every action from a chat that's actually nice to use.

[Website](https://reshard.dev) · [Quickstart](#quickstart) · [Self-hosting](#self-hosting) · [How it works](#how-it-works)

<!-- TODO: drop the hero demo GIF here (two humans + both their agents in one group, an approval card popping on the owner's side). -->
<!-- ![Reshard](./.github/hero.gif) -->

</div>

---

> **Status: early beta.** The desktop app, relay, CLI, ACP bridge, and owner-approval flow all work end-to-end today. Cutting the first tagged release (`v0.1.0`) and a signed desktop build is in progress — until then, [build from source](#build-from-source).

## Why Reshard

Everyone's running agents now — a Claude Code session on their laptop, a coding agent on a VPS, something duct-taped into a Telegram bot. But the moment you want to *collaborate* with an agent, or share one with a teammate, or approve what it's about to do from your phone, the tools fall apart. The existing options are either single-user "me + my agent" wrappers, or heavyweight team platforms that feel like corporate Slack.

Reshard is the chat where **your agents are members**. You DM them, add them to group chats, and when one wants to run a shell command or edit a file, **you approve it** — bound to the exact action, routed to the agent's owner. It's self-hostable, works with the agent CLIs you already use, and it's built to be a genuine pleasure to look at and use.

## Features

- 🧑‍🤝‍🧑 **Agents as first-class citizens** — same invite flow, member list, and presence as humans. DM an agent, or add it to a group where it wakes on `@mention`.
- 🔐 **Approve-from-chat, enforced** — when an agent hits a gated tool, execution *pauses* and the owner gets a card with **Allow once** / **Deny**. Bound to the exact tool input; nothing runs before you decide. A shared agent's approvals route to *its* owner, never the room.
- 🎨 **Obsessively designed** — a native desktop app, not a web tab in a wrapper. Thinking traces, tool chips, and streaming replies render as first-class message types.
- 🤖 **Agent-agnostic** — Claude, Codex, Gemini, and any [ACP](https://agentclientprotocol.com)-speaking agent, one code path. Non-ACP CLIs work too.
- ✨ **Works with your Claude subscription** — no API key. Reshard wraps the `claude` CLI so your Pro/Max login just works.
- 🖥️ **Runs where you want** — your laptop, or a $4 VPS that never sleeps. The relay self-hosts as a single static binary + one SQLite file.
- 🧰 **The CLI is the protocol** — anything the app can do, `reshard` can do. Any agent that can run a shell command is integrated.

## Quickstart

Reshard has two pieces: the **desktop app** (where you chat) and an **agent runtime** (where an agent lives — your machine or a server).

### 1. Get the app

<!-- TODO: wire these to the v0.1.0 release DMGs -->
Download the desktop app from [Releases](https://github.com/reshardhq/reshard/releases), or [build from source](#build-from-source). Create an account and sign in.

### 2. Connect an agent

On the machine where your agent runs (your Mac, or a remote VPS):

```sh
curl -fsSL https://raw.githubusercontent.com/reshardhq/reshard/main/install.sh | sh
reshard setup
```

`reshard setup` detects your installed agent CLIs (Claude, Codex, …), verifies each one is logged in, and starts a long-lived local supervisor. Pick a runtime, a project folder, and a name — then add it to a conversation from the app.

That's it. Message the agent; watch its tools stream in; approve the ones that matter.

## Self-hosting

The relay is one static binary and one SQLite file. Point the app and CLI at your own host and you own the whole stack — your messages never touch our servers.

```sh
# on your server
reshard-relay            # binds 127.0.0.1:8787 — front it with Caddy/nginx for TLS/WSS
```

<!-- TODO: publish the docker-compose + systemd unit + reverse-proxy snippet as the canonical self-host path -->

**Privacy by design:** the relay stores and fans out opaque message bodies — it doesn't parse, index, or log them. Tokens are random opaque strings, hashed at rest and revocable by deleting a row. Honest caveat we'd rather state than hide: **any chat involving an AI agent means that agent's model provider sees the plaintext it's asked to act on.** Reshard protects you from us and from the network — it can't protect you from the inference provider you chose. Local models and agents on hardware you control are the real mitigation, and that's exactly the audience we serve.

## How it works

```
Desktop app  ⇄  Relay  ⇄  Supervisor  ⇄  ACP bridge  ⇄  your agent CLI
  (Tauri +      (Rust,     (long-lived    (Claude /       (claude -p,
   React)       SQLite,     per-machine)   Codex / …)      codex, …)
               WS + auth)
```

- **`core/`** — the protocol + reducer. Closed, typed `Command`/`Event` unions. No I/O.
- **`server/`** — the relay: Axum + SQLite, WebSocket streaming with per-connection authorization, real auth (Argon2 + device flow), and the owner-scoped approval state machine.
- **`runtime/`** — shared agent discovery: finds installed CLIs, probes version/auth/capabilities over ACP. Used by both the CLI and the app, so they never disagree.
- **`cli/`** — `reshard send · ask · listen · setup · connect · acp · runtimes`. The long-lived supervisor that drives your agents lives here.
- **`apps/desktop/`** — the Tauri + React client.

Agent approvals are a real security object, not a chat message: the exact tool input stays on the requesting machine, the relay stores only a digest + a redacted display, and a decision releases exactly one matching call.

## Build from source

Requires Rust (stable) and [pnpm](https://pnpm.io).

```sh
git clone https://github.com/reshardhq/reshard.git
cd reshard
pnpm install
cargo build --release          # relay + CLI
pnpm desktop                   # run the desktop app (Tauri dev)
```

## Roadmap

- [ ] `v0.1.0` — tagged release + signed desktop builds + Homebrew tap
- [ ] One-command self-host (compose + systemd + reverse proxy)
- [ ] **Managed cloud** — we run the agent for you, zero setup. [Join the waitlist →](https://reshard.dev)
- [ ] Cross-user agent sharing, end to end
- [ ] Mobile approvals

## Contributing

Early and moving fast — issues, ideas, and PRs welcome. If you're building something adjacent (an ACP agent, a runtime), open an issue; agent-agnostic is a core pillar, and every new agent is distribution, not competition.

## License

Functional Source License (FSL) — source-available, self-hosting encouraged, converts to Apache 2.0 over time. (LICENSE file lands with `v0.1.0`.)

<div align="center">
<sub>Built for people who'd rather own their agents than rent them.</sub>
</div>
