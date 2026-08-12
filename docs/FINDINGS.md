# Findings

Living doc of product insights discovered while building. Newest last.

## 2026-08-01 — Day one

### The four pillars
1. **Beautiful design (UI + UX)** — the only moat left in the AI agent era.
2. **AI-native** — agents are first-class citizens: same invite flow, presence, and member list as humans. Not bolted-on bots.
3. **Agent-agnostic** — any model, any CLI (Claude, Codex, Kimi, OpenClaw). Every new agent launch is distribution, not competition.
4. **First-class CLI** — the CLI can do everything the app can. The CLI is the protocol: any agent that can run a shell command is integrated.

### IA: WhatsApp, not Slack
Started with Slack-shaped channels; pivoted to the messenger model — one flat,
drag-sortable list of conversations: **DMs with agents** (agent always replies,
no @mention needed) and **created groups** (mention-triggered). Simpler mental
model, and it's what the target market already does (OpenClaw users live in
Telegram). Channels can return later as dressed-up groups for teams.

### Agentception: the app is agent-controllable
Every UI action routes through one command bus (`commands.ts`), so an in-app
brain can drive the app itself — ⌘K one-shot commands, ⌘J "caddy" for
multi-step plans. The command registry's schemas double as LLM tool
definitions, so any new command is automatically speakable. Agent actions
visibly "press" the UI (touch highlight + beat) — trust through visibility.

### THE finding: agents are shareable — cross-user agent chat
You can add **your friend's agent** to a group chat. This changes everything:

- **Viral loop**: sharing an agent drags in its owner. "Add your agent to the
  group" is an invite mechanic no chat app has had.
- **Agents become social objects**: something you tune, show off, and lend.
  Status mechanics driving infrastructure adoption.
- **Safety falls out of the existing `ask` primitive**: a lent agent's
  approvals route to its *owner's* phone, not the group. Owner keeps control,
  host stays safe, audit trail records both.
- **Architecture implication**: agent identity must be global from day one —
  name + owner + token, portable across workspaces. Design the relay (phase 2)
  around this; retrofitting federation is pain.
- **The launch demo**: two humans + both their agents in one group; human A
  asks human B's agent for something; the approval pops on B's phone.

### Design language findings
- Stock shadcn dark palette beats custom charcoal (lighter sidebars against a
  deeper background reads less "black slab").
- Monochrome dither-kit pixel avatars = the identity system. Unique pattern
  per name, one gray tone, hairline hue-free frame. Personality without color
  noise; matches the dithered chart/primitive aesthetic.
- Density: 85% root font, -0.011em tracking, 38px integrated titlebar,
  status bar with live shortcut hints. Terminal-adjacent, not terminal-cosplay.
- The 9 ported AI primitives (thinking trace, tool chips, streaming text,
  approval flows, task rows, code block, insights, recommendation) read as
  native *message types*, not embeds — this is what "AI-native chat" looks
  like concretely.

### Naming (parked, marinating)
Frontrunner: **Reshard** (reshard.dev secured). Bench: Resonar, Recrew, Relay,
Remuse. Constraint: starts with "Re". Working name in code: `agentchat`.

### Licensing (decided direction, not yet applied)
FSL (Sentry's Functional Source License) — source-available, self-hosting
fine, competing commercially prohibited, converts to Apache after 2 years.
Decide formally before any external contributions arrive.

### Status protocol: how agents report thinking/actions (designed, not yet built)
Three tiers feeding one event stream:
1. **Bridge parsing** — `agentchat up` adapters translate each CLI's native
   structured output (e.g. `claude -p --output-format stream-json`) into relay
   events. Zero agent effort.
2. **Explicit verbs** — `agentchat status thinking|tool|progress|done` for
   anything that can shell. Bridge auto-emits working:true/false around runs.
3. **Rich streams** — `agentchat stream` (JSONL on stdin) and `agentchat mcp`
   (post_status/send_message/ask as MCP tools).

Unifying concept: the **turn** — turn.start → status/tool events append →
final text → turn.end. One evolving message in the UI: working loader →
ThinkingState → ToolChips → text, collapsing to "Thought for Ns ›".
The showcase primitives are the renderer for this wire format.

**CLI vs MCP decision:** CLI is the base protocol — zero config, universal
(anything that can shell). `agentchat mcp` is a thin optional wrapper over the
same command bus for MCP-native agents. Never require MCP.

### Hermes Agent (Nous Research) teardown — validation from the incumbent
Studied hermes-agent (the VPS-agent-in-Telegram archetype our users duct-tape):
- Their agent fires `tool.started`/`tool.completed`/`_thinking` callbacks with
  tool_name/preview/args/duration — same vocabulary as our turn events.
- Their gateway DEGRADES this telemetry into borrowed platforms: Slack status
  line ("is running pytest…"), Telegram typing bubbles, progress via message
  edits, per-chat verbosity modes (off/log/all, /verbose). Hundreds of lines
  of per-platform capability fallbacks (`supports_status_text`,
  `supports_code_blocks`).
- **Strategic read: their gateway complexity is the tax our native client
  eliminates.** We render the same events as first-class UI (ThinkingState,
  ToolChips) instead of squeezing them through typing indicators.
- Steal: status-phrase builder from tool+args, per-chat verbosity setting,
  duration on tool.completed, pausing the working indicator during approval
  waits, async wake-after-turn-ends for background process completions.

### Auto-updates (phase 4, decided)
tauri-plugin-updater + signed manifests on GitHub Releases via tauri-action CI.
Background download, "update ready — restart" chip in the status bar,
auto-apply on quit. Frontend-only changes = small silent updates. CLI updates
separately (`agentchat upgrade` / npm / brew); relay can nudge stale bridges.
Do NOT remote-load the webview as a shortcut.

### Two integration modes — bridge vs mirror (2026-08-03)
A Claude plugin is a great *on-ramp*, never the foundation: pillar 3 dies if
only one agent works. The CLI stays the protocol. But thinking it through
surfaced a second product mode we hadn't named:

- **`reshard up` — bridge.** reshard owns the agent, invoking it headless on
  each message. For unattended VPS boxes. *"My agent lives here."*
- **plugin / hooks — mirror.** The agent runs where the user already runs it,
  interactively; reshard watches and mirrors. *"My laptop session is visible
  from my phone."*

Mirror is far cheaper to build and solves the gap hit while dogfooding: Claude
Code hooks (`PostToolUse` → `reshard status tool`, `Stop` → `reshard send`) emit
the whole turn stream with ~20 lines of config and no bridge process. It also
ships as an installable plugin into the largest agent population.

Build order: mirror hooks → `up` → both on the same CLI.

## 2026-08-08 — ACP is the bridge; Buzz teardown

### Decision: adopt ACP (Agent Client Protocol)
The agent-agnostic bridge is a solved problem — **ACP**, Zed's open standard
(JSON-RPC/stdio). One client drives Claude/Codex/Gemini/Cursor/…; `session/update`
gives streaming tool telemetry, `session/request_permission` gives the approval
hook. We use the **official Rust SDK** (`agent-client-protocol` 2.0.0). Built it:
see **`docs/AGENT_BRIDGE.md`** for the full architecture. Highlights:

- **`claude-code-acp`** — our own Rust ACP adapter wrapping `claude -p` →
  **subscription auth, no Node, no API key**. The official `claude-agent-acp`
  wraps the Agent SDK → API-key-only (Anthropic blocks subscription for the SDK).
  Wrapping the CLI is the only subscription path; **Buzz doesn't have it**.
- **Session continuity** fixed by persisting Claude's `session_id` per chat and
  `--resume`-ing. **`--cwd`** binds an agent to a project dir (reads its docs).
- The first-class **approval card** and local ACP broker are implemented. Claude
  now fails closed without bypass permissions; its enforceable private
  `--permission-prompt-tool` bridge is now wired for subscription-backed
  Claude runs, with capability gating for older CLIs.

### The bridge is commoditising (competitive)
Products already do "control your coding agent from chat with approvals":
OpenACP (OSS, ACP-based, now gone), Omnara, Happy (`slopus/happy`), cc-connect.
All **single-user "me + my agent."** Our moat is the multiplayer layer: agents as
members alongside multiple humans, cross-user agent sharing, projects-not-channels,
cloud-hosted, beautiful UI. The bridge is table stakes; don't treat it as the moat.

### Buzz (block/buzz) teardown — the closest prior art
Block's agent-chat, 269k-LOC Rust monorepo, ACP-based (hand-rolled, not the SDK),
built on **Nostr**. What they get right and what to copy:
- **Long-lived agent process, session-per-channel** (`HashMap<channel → session_id>`,
  `pool.rs`) — reuse the ACP session per chat; don't spawn per message. (We fixed
  the symptom with disk-persisted session ids; their in-memory pool is the scale-up.)
- **Agent publishes its own messages via a tool call**, signed by its own key —
  reasoning/tool-calls stay invisible; only published messages are chat events.
- **Observer frames** (owner-encrypted, ephemeral) for "agent working" telemetry
  off the timeline; typing + turn-liveness indicators.
- **Persona file** (`.persona.md`: frontmatter + markdown-as-system-prompt,
  `runtime`/`model`/declarative `triggers`); pack manifest with layered defaults.
- Owner-attested connect (their invite mechanic); rotate-session-after-N-turns to
  bound context.

**Buzz's weaknesses = our openings** (do NOT copy):
1. **Nostr as identity/storage/authz** — root of their pain: `nsec` = identity with
   **no recovery**, a 4.3k-LOC pairing stack just for a 2nd device, moderation-as-
   event-kinds, relay-as-central-signer, per-message signature verification. Yet
   it's fully centralised anyway. Normal login beats QR/SAS key-pairing day one.
2. **Channels-everywhere.** Their "projects" are mostly *designed, not built*,
   bolted onto channels. Exactly the Oliur demand-signal gap — projects-first wins.
3. **No human tool-approval.** Their marquee "approve the merge" is a **stub that
   fails the run** (WF-08); their agent path auto-executes tools with **no gate**
   and a deliberately network-widened sandbox. A polished approve-from-your-phone
   card is an immediate, defensible differentiator — the thing Block couldn't ship.
4. **Kitchen-sink monolith** — git host + WebRTC voice SFU + tunnel + 127 event
   kinds + 50 flags/47 env vars per crate. Scope discipline + design is our edge
   (match useful behaviour in ~1/10 the code, spend the rest on UI).
5. **Stubs sold as features** — rate limiter is a no-op `AlwaysAllow`; audit log is
   keyless. If we add these, actually enforce them.

We're already **ahead** on two axes: durable continuity (we survive a process
crash; Buzz loses chat memory) and **subscription Claude** (they're API-key-only).

### The permission model, straight
Per-tool "approve from chat" is genuinely hard — **even Block punted.** The clean
design is now implemented for native ACP: don't skip permissions or
blanket-allow; route `request_permission` to the human (the owner for a lent
agent), bind the decision to the exact input digest, and release it once. Claude
uses the same permission-prompt MCP adapter as native ACP, with subscription
credentials kept out of Claude's own environment.
