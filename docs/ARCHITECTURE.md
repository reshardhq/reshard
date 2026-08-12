# Architecture

> Status: **proposed** — under review. Nothing here is built yet except the
> desktop client. Decisions marked ⚠️ are still open.

## The thesis

> Everything in this repo is a thin client of one protocol and one pure function.

A chat app is, honestly, `state = events.reduce(apply)`. The protocol and that
reducer live in exactly one place; the server, the CLI, and every client are
adapters around them. Infrastructure choices are drivers behind two interfaces,
so "self-hosted on a Raspberry Pi" and "hosted cloud tier" differ by one env
var, not by a fork.

---

## Repo layout

```
reshard/
├── core/     protocol + reducer + validation. no I/O, no deps.
├── server/   the relay
├── cli/      reshard send · listen · ask · up
└── app/      desktop — Tauri + React
```

Four directories, one job each. A stranger should understand the system in ten
seconds; that is a design requirement, not a nicety.

The TypeScript types are a *generated artifact*, not a package:
`core/` → `app/src/protocol.gen.ts`, committed, with a "do not edit" header and
a `just gen` to refresh it. One file, no ceremony.

**Later, not now:** `mobile/` (Expo, native views, same generated types) when
push notifications matter. There is no `web/` and no shared `ui/` package —
views live in `app/`. Extracting a shared `ui/` is a refactor to do *when* a
second DOM client exists, not in anticipation of one.

Worth knowing: `app/`'s frontend is already a plain Vite React build that Tauri
wraps, so shipping a web version later is a deploy target rather than a rewrite.
The option stays open for free.

---

## Stack decision

| Piece | Choice | Why |
|---|---|---|
| `core`, `server`, `cli` | **Rust** | Single static binaries: `curl \| sh` installs the CLI on any VPS with no runtime. 3–5× lower memory per WebSocket. Relay can ship as a Tauri sidecar for zero-setup local mode. |
| `app` (and `mobile` later) | **TypeScript** | Where the UI work is, and where iteration speed matters most. |
| Protocol sharing | **ts-rs / typeshare** | Types defined once in Rust, generated into `app/src/protocol.gen.ts`. One source of truth across the language boundary, zero packages. |

**Server crates:** `axum` (HTTP + WS) · `tokio` · `sqlx` (SQLite + Postgres from
one API) · `serde`.
**CLI crates:** `clap` (args, help, completions) · `ratatui` + `crossterm`
(live dashboards) · `indicatif` (progress) · `inquire` (prompts) · `owo-colors`.

**Alternative considered:** all-TypeScript (Hono + `ws` + `node:sqlite`). Faster
to build and gives a literally-shared reducer, but requires Node on every agent
box, costs 3–5× the RAM per connection, and loses the sidecar story. Revisit if
Rust velocity becomes the bottleneck for phase-3 adapter churn.

**Client-side state:** clients do *not* re-implement the domain reducer. The
server is authoritative; clients apply events to a view model (append message,
resolve ask, set presence) — thin by nature. Shared event fixtures in `core/`
run against both implementations as a conformance suite.

---

## The protocol

Clients send **commands**. The server emits **events**. Both are closed, typed
unions — roughly 150 lines total.

```rust
enum Command {
  Send    { chat: Id, text: String, idem: String },
  Ask     { chat: Id, prompt: String, options: Vec<String>, ttl: Option<u32> },
  Resolve { ask: Id, option: String },
  Status  { chat: Id, state: StatusState, label: String },
}

enum Event {
  Message    { chat: Id, seq: u64, author: Id, .. },
  AskOpened  { .. },
  AskResolved{ .. },
  AskExpired { .. },
  Status     { .. },        // ephemeral — never persisted
  Presence   { .. },
}
```

### The turn

An agent's reply is a **turn**: `turn.start` → status/tool events append →
final message → `turn.end`. The UI renders one evolving message — working
loader → thinking trace → tool chips → text, collapsing to "Thought for 4s ›".

Only the turn *summary* is persisted. The 20–50 status events inside it are
fanned out and discarded.

### CLI ↔ protocol ↔ UI

The CLI is not a wrapper around an API; it is the API.

| CLI | Command | UI |
|---|---|---|
| `reshard send -m` | `Send` | composer |
| `reshard ask` | `Ask` → `Resolve` | ApprovalCard |
| `reshard status` | `Status` | ThinkingState / ToolChips |
| `reshard listen` | — (event stream) | message list |

---

## The relay

### Chats are actors

Not a router in front of a database — an in-process map of small actors, one
per chat.

```rust
struct ChatRoom {
    sockets: HashSet<SocketId>,
    seq: u64,                 // monotonic, owned by this actor alone
    inbox: mpsc::Receiver<Command>,   // commands processed one at a time
}
```

Serialised command handling per chat buys, with no locks and no distributed
coordination:

- monotonic per-chat sequence numbers
- race-free ask resolution and unread counters
- a structure that maps **1:1 onto a Cloudflare Durable Object** if the hosted
  tier ever moves there

### Two rules that define the system's efficiency

1. **Ephemeral by type.** `Status` and `Presence` events are broadcast, never
   written. The firehose never touches disk.
2. **Typed backpressure.** When a socket's send buffer is full, drop `Status`
   events; never drop `Message` or `Ask`. Slow clients lose telemetry, never
   history.

### Wire

```
client → wss://relay/stream          token via subprotocol
client → { t:"hello", cursors:{ c_dm_claude: 412 } }
server → …replays events after each cursor, then live tail
client → { t:"send", chat, text, idem }
server → { t:"message", chat, seq, … }
         ping/pong 30s; dead sockets reaped
```

Reconnect = `hello` with last seq per chat. Idempotency keys make retries free.
That is the entire sync algorithm — no CRDTs, no OT, because chat doesn't need
them.

---

## Storage

**SQLite by default. Postgres as a driver.**

SQLite is right for the first year and for every self-hoster: writes are ~12/sec
at 10k users (SQLite does thousands), reads are microseconds with no network
hop, backup is a file copy, and self-hosting is *one container and one file*.
Postgres exists for the day the hosted tier runs multiple relay nodes.

```sql
events(id, chat_id, seq, kind, author_id, body_json, created_at)
chats(id, kind, name, avatar_seed, created_at)
members(id, kind, name, owner_id, bio)      -- owner_id ⇒ cross-user agents
chat_members(chat_id, member_id, role)
tokens(id, subject_id, kind, hash, scopes, revoked_at)
outbox(id, kind, payload, attempts, next_try_at, done_at)
```

Messages *are* events; state is derived. Asks are events
(`opened`/`resolved`/`expired`), so the compliance-grade audit trail is
inherent rather than a feature to bolt on later.

Tokens are opaque random strings stored as SHA-256 hashes — no JWT, no auth
SaaS, revocable by deleting a row. `members.owner_id` and `tokens.subject_id`
carry the global agent identity that cross-user agent sharing needs
(see FINDINGS), designed in from row one.

---

## Ports and drivers

```
server/  Storage   → memory · sqlite · postgres
         Bus       → local · redis
         Notifier  → apns · fcm
clients/ Transport → memory · websocket · http
```

Each driver is a small file. Two consequences worth stating plainly:

- Swapping infrastructure is visibly *one file*, not a config maze.
- The desktop app's dev mode is not a mock — it is the real client on the
  `memory` transport. No mock branch exists anywhere in the codebase.

---

## Deliberate omissions

| Not used | Why | Add it when |
|---|---|---|
| **Redis** | Fan-out between processes is the only problem it solves; we run one process | relay node #2 exists |
| **Message queue** | The event log with per-client cursors *is* a durable queue. External side effects use a transactional `outbox` table | push volume outgrows one drain loop, or work must survive relay downtime |
| **ORM** | Two hand-written SQL drivers, ~120 lines each | never, probably |
| **Cron** | Timers + an `expires_at` sweep on boot | scheduled work outlives the process |
| **Microservices** | One binary, one deploy | more than one team |
| **GraphQL / socket.io / auth SaaS** | Closed protocol, one transport, opaque tokens | — |

Naming omissions with triggers is the point: each is a decision, not an
oversight.

---

## Hosting

| Piece | Where | ~Cost |
|---|---|---|
| Relay + SQLite | Hetzner CX22 (2 vCPU / 4 GB), Ashburn | €4/mo |
| DNS · TLS · DDoS · WS proxy | Cloudflare | €0 |
| Binaries, `install.sh`, attachments | Cloudflare R2 + GitHub Releases | pennies |
| Landing + docs | Vercel or CF Pages | €0 |
| Magic-link email | Resend | €0 |
| Push | APNs / FCM (keys only, no hosting) | $99/yr Apple |
| Backups | `sqlite3 .backup` cron → B2, or Litestream → R2 | ~€3/mo |
| Self-hoster ingress | Cloudflare Tunnel (documented, hardened) | €0 |

**Total ≈ €10/month** to run the product.

### Capacity

Connections are the binding constraint, and agent bridges never disconnect:

```
sockets ≈ (0.15 × users) + (1.0 × agents)
Rust WS ≈ 5–15 KB/conn  →  4 GB ≈ 100k+ sockets
                        →  ~40k users with ~80k always-on agents
```

Traffic is nowhere near the limit at that size (~80 events/sec average). What
breaks first, in order:

1. **Disk** — ~10–20 GB/month of events at 10k users. Fix: a volume, or
   *history retention as a pricing tier* (free = 30 days).
2. **Connections** — vertical scaling covers it to ~50k users on one box.
3. **Single point of failure** — the real reason to go multi-node, and the
   moment `Bus → redis` and `Storage → postgres` get switched on.

| Users | Setup | Infra |
|---|---|---|
| 0 – 40k | 1 box, SQLite, local bus | ~€10 |
| 40k – 150k | bigger box + volume | ~€40 |
| 150k+ | 2–3 nodes + Redis + managed Postgres | ~€200–400 |

### Cloudflare Durable Objects — documented future path

DOs are one-per-chat stateful actors with WebSocket hibernation (idle sockets
cost nothing) and per-object SQLite. That is the same shape as our `ChatRoom`
actor, which is deliberate. **Not adopted now** because a DO relay cannot be
self-hosted, and self-hosting is the distribution engine. If the hosted tier
ever outgrows vertical scaling, porting it is a driver swap — `core/` and the
protocol are untouched.

---

## Deployment

```yaml
services:
  relay:
    image: reshard/relay          # single static binary
    volumes: [./data:/data]      # one SQLite file
    ports: ["8080:8080"]
```

One service. The compose file published for self-hosters *is* what runs in
production — the OSS install path cannot rot, because it is the product.

---

## Security and privacy

People will ask "can you read my messages?" before they ask anything else.
The answer has to be structural, not a promise.

### Layers, in order of what we owe users

| Layer | Status | Protects against |
|---|---|---|
| **TLS everywhere** (`wss://`, HTTPS) | required from day one | network attackers, coffee-shop Wi-Fi |
| **Encrypted at rest** — full-disk on the relay host, encrypted backups | required before the hosted tier takes a single user | stolen disks, leaked backups, a snapshot on someone's laptop |
| **No message bodies in logs. Ever.** Structured logs carry ids, never `text` | enforced in review | the most common real-world leak, by far |
| **Self-hosting** | already shipped as pillar 3 | *us* — the strongest honest answer available today |
| **End-to-end encryption** | designed for, not yet built | a compromised or subpoenaed relay |

### The constraint that keeps E2EE cheap later

**The relay must never need to read `text`.** It stores, sequences, and fans
out opaque bodies; it does not parse, index, or interpret them. Today that is
already true by accident — keep it true on purpose, and encrypting bodies later
is additive rather than a rewrite.

Practically, that forbids: server-side search over message content, push
payloads containing message text, link unfurling on the relay, and any
server-side AI features over history. Each of those would have to move
client-side, which is where they belong anyway.

### Why E2EE is genuinely harder here than in Signal

In a normal messenger, every endpoint is a human device holding keys. Here one
participant is a **headless agent on a VPS**:

- The agent needs the chat key on a box that is usually less hardened than a phone.
- **Cross-user agent sharing** — the feature the whole growth model rests on —
  becomes a key-distribution problem across owners.
- Membership churn (adding an agent to a group) means rekeying.

The shape that works: a per-chat symmetric key, wrapped to each member's
device/agent public key, rotated on membership change — i.e. **MLS (RFC 9420)**,
which exists precisely for group churn. An agent gets its keypair at
`reshard join`; its owner's device wraps the chat key to it. Approvals still
route to the owner, unchanged.

### The honest caveat we should publish, not hide

**For any chat involving an AI agent, the model provider sees plaintext.** The
agent has to send message text to Anthropic/OpenAI/wherever to respond to it.
E2EE protects users from *us* and from the network — it cannot protect them
from the inference provider they chose, and no chat app can claim otherwise.

Saying this plainly in the docs earns more trust than a padlock icon, and it
points at the real mitigation: local models, or agents that run on hardware the
user controls. Which is, conveniently, exactly the audience we serve.

---

## Open decisions ⚠️

- **Binary name.** `reshard` (6 chars) vs something shorter. It bakes into
  muscle memory, docs, and every agent's config file.
- **Licence.** FSL leaning (see FINDINGS) — decide before external
  contributions arrive.
- **Client reducer in WASM?** Compiling `core/` to WASM would make the shared
  logic literal rather than conformance-tested. Elegant for web/desktop, painful
  for React Native. Deferred.
- **Region.** Ashburn (closest to early adopters) vs Singapore (closest to you).
- **E2EE timing.** Ship at-rest + self-host first, or hold the hosted tier until
  MLS lands? Leaning: ship, but never let a server-side feature depend on
  reading `text`, so the door stays open.
