# Runtime onboarding, supervision, and human approvals

> Status: **Phases 1–4 code implemented; live Claude conformance pending; Phases 5–6 proposed**.
> Supersedes the per-message shell lifecycle in `cli/src/up.rs`, the anonymous
> desktop bootstrap in `apps/desktop/src/lib/auth.ts`, and the default
> `--dangerously-skip-permissions` Claude path in
> `cli/src/bin/claude-code-acp.rs`.

## Outcome

Installing Reshard should produce one obvious setup flow:

```text
install Reshard
  -> authenticate this machine
  -> detect installed agent CLIs
  -> verify each CLI and its own login
  -> choose one or more runtimes
  -> choose projects and names
  -> start one long-lived local supervisor
  -> control the agents from Reshard chat
```

When an agent wants to perform a gated action, execution pauses and the agent's
owner gets a Reshard card with **Allow once** and **Deny**. No tool runs before
that decision. A timeout, disconnect, stale request, process crash, or invalid
response is a denial.

This keeps the parts of Buzz that work well—runtime discovery, readiness
checks, a managed ACP pool, and per-chat sessions—without copying its identity
model, its hand-written ACP wire, or its missing human-approval enforcement.

## Non-negotiable decisions

1. **Reshard account auth is real.** Production never silently creates an
   anonymous workspace.
2. **The CLI is interactive by default.** Flags and JSON remain available for
   automation, but a new user should not need to know provider names or paste a
   large command.
3. **Detection precedes selection.** Reshard presents what is installed, what is
   logged in, what needs an adapter, and what is unsupported.
4. **One runtime engine.** The CLI and Tauri app consume the same Rust runtime
   catalog and readiness code. They must never disagree about what is installed.
5. **ACP stays the provider seam.** Reshard uses the official Rust ACP SDK for
   ACP agents and small provider shims only where a vendor CLI needs one.
6. **Claude subscription auth stays supported.** Reshard continues to wrap the
   user's `claude` CLI rather than replacing it with an API-key-only SDK path.
7. **Ask is the default permission policy.** Reshard never adds
   `--dangerously-skip-permissions` during normal setup or execution.
8. **Approval is enforced at the tool boundary.** A chat message saying
   "approved" is not an approval; the exact pending tool call must be released
   by the local runtime.
9. **Per-chat isolation remains.** Provider context is keyed by
   `(agent_id, chat_id)`. Context must not leak between a DM and a group.
10. **The owner approves.** A lent agent's requests go to its owner, not to
    every member of the room and never to the agent itself.

---

## What existed at the design baseline

Reshard has useful pieces, but not the complete product flow:

- `reshard connect` requires the caller to choose `--provider` up front.
- Detection is a single `PATH` existence check for the selected binary.
- There is no provider catalog, version probe, login probe, or capability report.
- The gateway launches a new shell/ACP/Claude process chain for every message.
- The desktop login form is present, but `initAuth`, `signIn`, and `register`
  call anonymous `/bootstrap`; the supplied email and password are ignored.
- `App.tsx` renders a blank surface while unauthenticated instead of rendering
  the existing `AuthPage`.
- Claude is launched with `--dangerously-skip-permissions`.
- The generic ask card works visually, but it is not a safe tool-approval
  primitive: an agent machine cannot open it, any chat member can resolve it,
  the submitted option is not checked against the offered options, and it is
  not bound to an exact tool input.

Buzz's desktop has the missing discovery shape: a catalog of known runtimes,
login-shell PATH recovery, separate `Available`/`AdapterMissing`/`NotInstalled`
states, parallel auth probes, preset ACP harnesses, and managed processes. In
Buzz this logic is tightly housed in the Tauri backend. Reshard will make it a
shared local runtime library so it also works on a headless server or VPS.

---

## User experience

### Canonical first run

Running `reshard` with no established configuration starts `reshard setup`.

```text
$ reshard setup

Reshard setup

  Account      Not signed in
  Machine      t31k-macbook

Open https://app.reshard.dev/device and enter: RBM-K7QP
Waiting for approval... approved as t31k@example.com

Detected agent runtimes

  [x] Claude Code  /opt/homebrew/bin/claude
      Installed · logged in with Claude · subscription compatible
      Reshard approval bridge supported

  [x] Codex        /opt/homebrew/bin/codex
      Installed · logged in · ACP adapter ready

  [ ] Kimi Code    /usr/local/bin/kimi
      Installed · login status unavailable · native ACP

  [ ] Hermes
      Not installed

Select runtimes to enable: Claude Code, Codex
Project folder for Claude Code: /Users/t31k/Projects/reshard
Agent name: claude-main
Permission policy: Ask in Reshard (recommended)

Installed and started the Reshard agent service.
Open Reshard to add claude-main to a conversation.
```

No password is typed into the CLI. `reshard auth login` uses a device/browser
flow and receives a revocable machine credential only after the signed-in user
approves it.

### Subsequent commands

```text
reshard setup                 # rerun the guided setup
reshard auth login            # authenticate or switch account
reshard auth status
reshard auth logout
reshard runtimes              # interactive catalog
reshard runtimes --json       # automation and Tauri consumption
reshard runtime enable claude
reshard runtime doctor claude
reshard agents                # configured local agents and status
reshard service status
```

Existing low-level commands remain available, but `connect`, raw `acp`, and
explicit adapter commands are advanced/debugging surfaces rather than the
primary onboarding path.

### Desktop first run

The Tauri app renders the existing authentication page when there is no valid
session. After login, it shows machines and their discovered runtimes.

On the same machine, Tauri invokes the shared runtime discovery library
directly. For a remote machine, it displays the inventory last reported by that
machine's Reshard supervisor. Provider credentials and raw local paths stay on
the machine; the relay receives only bounded readiness metadata.

---

## Real authentication

### Human sessions

Add production endpoints for registration and login. `/bootstrap` becomes
development-only and is rejected in production.

```text
POST /auth/register
POST /auth/login
POST /auth/logout
POST /auth/refresh
GET  /auth/me
```

- Passwords are stored with a memory-hard password hash.
- Session and refresh tokens are random, hashed at rest, expiring, and
  individually revocable.
- Authentication endpoints are rate-limited.
- Tauri stores its token in the operating-system credential store. Browser
  development may use web storage, but production Tauri must not treat
  `localStorage` as the credential vault.
- Sign-out revokes the server session and removes the local credential.

### Machine authorization

CLI authentication is an OAuth-style device flow:

```text
CLI                       relay                         Tauri/web
 | POST device/start        |                              |
 |<-- code + verify URL ----|                              |
 |                          |<-- signed-in user approves --|
 | poll device/token ------>|                              |
 |<-- machine credential ---|                              |
```

The credential is bound to a machine record containing owner, display name,
created time, last seen time, and revocation state. The CLI writes it with mode
`0600` (or the platform credential store where available).

Machine credentials are scoped. They may register local runtime inventory,
operate only their owner's agents, request approvals for those agents, and
consume decisions addressed to those requests. They may not act as a human,
approve their own request, or read owner-private events unrelated to their
agents.

---

## Shared runtime catalog

Create a small workspace crate, tentatively `runtime/`, used by both `cli/` and
the Tauri shell. It contains discovery and readiness only; it does not know
about React or chat rendering.

```rust
struct RuntimeDefinition {
    id: RuntimeId,
    label: &'static str,
    commands: &'static [&'static str],
    launch: LaunchStrategy,
    auth_probe: Option<Probe>,
    install: InstallMetadata,
    capabilities: RuntimeCapabilities,
}

struct RuntimeReport {
    id: RuntimeId,
    binary_path: Option<PathBuf>,
    version: Option<String>,
    availability: Availability,
    auth: AuthStatus,
    adapter: AdapterStatus,
    capabilities: RuntimeCapabilities,
    diagnostics: Vec<Diagnostic>,
}
```

Initial availability states:

- `Ready`
- `LoginRequired`
- `AdapterMissing`
- `UnsupportedVersion`
- `ConfigInvalid`
- `NotInstalled`
- `ProbeFailed`

Initial capability fields:

- native ACP or adapter-backed
- subscription-compatible
- resumable sessions
- enforceable tool approvals
- cancellation support
- model switching
- maximum parallelism
- execution locus (local process, remote daemon, or external service)

### Detection rules

1. Recover the user's login-shell PATH. GUI applications often have a smaller
   PATH than Terminal.
2. Resolve an absolute executable path and canonicalize it before probing.
3. Probe versions and auth status with bounded timeouts and no interactive stdin.
4. Run independent provider probes concurrently.
5. Cache reports briefly, but invalidate after install, login, PATH change, or
   explicit refresh.
6. Never execute a frontend-supplied arbitrary path. Tauri may probe only a
   path returned by trusted discovery.
7. Never automatically execute an installer. Show the exact action and require
   confirmation first.

### Initial provider catalog

| Runtime | Discovery | Launch strategy | Auth/readiness |
|---|---|---|---|
| Claude Code | `claude` | Reshard's subscription-compatible Claude ACP shim | `claude auth status`; permission-prompt capability probe |
| Codex | `codex` + ACP adapter | ACP | `codex login status`; adapter/version probe |
| Gemini | `gemini` | `gemini --experimental-acp` | bounded native ACP initialize probe |
| Kimi Code | `kimi` | `kimi acp` | bounded native ACP initialize probe |
| Hermes | `hermes-acp`, with legacy fallback clearly marked | ACP when available | initialize probe; legacy mode is capability-limited |
| OpenCode | `opencode` | `opencode acp` | initialize probe |
| Custom | user definition | explicit command and args | ACP initialize probe |

The catalog is data-driven. Adding an agent should normally be one definition
plus conformance fixtures, not branches across the CLI, daemon, and UI.

---

## Long-lived agent supervisor

The installed service is the local execution authority. It replaces the
per-message `sh -c` gateway lifecycle.

```text
relay event stream
       |
       v
ReshardSupervisor
  |- RuntimeCatalog
  |- AgentWorker(agent_id, runtime, project)
  |    |- persistent ACP connection
  |    |- ChatSession(chat A) -> queue, provider session, active turn
  |    `- ChatSession(chat B) -> queue, provider session, active turn
  |- ApprovalBroker
  `- observer/telemetry publisher
```

```rust
struct AgentWorker {
    definition: AgentDefinition,
    adapter: AdapterProcess,
    sessions: HashMap<ChatId, ChatSession>,
    restart: RestartPolicy,
}

struct ChatSession {
    provider_session_id: Option<String>,
    queue: VecDeque<QueuedPrompt>,
    active_turn: Option<ActiveTurn>,
    pending_approval: Option<ApprovalId>,
    checkpoint: SessionCheckpoint,
}
```

Runtime rules:

- One active prompt per `(agent, chat)`; later messages queue in order.
- Different chats may run concurrently within the runtime's parallelism cap.
- Session identifiers are held in memory and checkpointed atomically to disk.
- A process crash restarts with bounded exponential backoff.
- Repeated crashes trip a circuit breaker visible in the app.
- Cancellation kills the active provider turn and cancels its approval.
- Shutdown drains or cancels deterministically; it never silently abandons a
  tool that may still be running.
- The relay owns identity and chat authorization. The supervisor owns local
  processes, paths, provider credentials, queues, and exact tool inputs.

ACP agents send `session/request_permission` directly into the ApprovalBroker.
Provider-specific shims must produce the same normalized request. An adapter
that cannot enforce the returned decision is marked unsupported under the
default Ask policy and is not started as if it were safe.

---

## Claude subscription and permissions

Claude remains a CLI-backed provider:

```text
Reshard ACP client
  -> claude-code-acp
      -> claude -p --output-format stream-json --resume <session>
```

The correction is how permissions are connected. Claude's supported
non-interactive gate is `--permission-prompt-tool`: an MCP tool Claude invokes
when a gated tool needs a human decision. Reshard supplies a private local MCP
server and passes it explicitly:

```text
claude -p ...
  --permission-mode default
  --strict-mcp-config
  --mcp-config <generated-reshard-only-config>
  --permission-prompt-tool mcp__reshard_permissions__request
```

There is no `--dangerously-skip-permissions` argument.

The generated settings overlay places state-changing tools such as shell
commands and file writes in `ask`. Claude's permission precedence evaluates
ask rules before allow rules, so a broad user/project allow rule cannot silently
bypass a Reshard-required card. Existing deny policies remain effective.

The local permission MCP process connects to the supervisor through a private
Unix socket (named pipe on Windows) using a short-lived per-run secret. It
cannot approve anything itself. It sends the exact tool name and input to the
ApprovalBroker, blocks, and returns either:

```json
{ "behavior": "allow", "updatedInput": { "the": "original input" } }
```

or:

```json
{ "behavior": "deny", "message": "The user denied this action in Reshard." }
```

Feature detection is mandatory. If the installed Claude version does not
support the permission prompt tool or the MCP broker cannot start, Claude is
reported as not ready under Ask policy. Reshard fails closed; it does not retry
with bypass permissions.

---

## First-class approval protocol

The existing generic `Ask` remains useful for ordinary questions, but tool
permission is a separate security object.

```rust
struct Approval {
    id: ApprovalId,
    owner_id: UserId,
    machine_id: MachineId,
    agent_id: AgentId,
    chat_id: ChatId,
    run_id: RunId,
    tool_call_id: ToolCallId,
    provider: RuntimeId,
    tool: String,
    display: ApprovalDisplay,
    input_digest: String,
    state: ApprovalState,
    expires_at: Millis,
    created_at: Millis,
    resolved_at: Option<Millis>,
    resolved_by: Option<UserId>,
}

enum ApprovalDecision { AllowOnce, Deny }
enum ApprovalState { Pending, Allowed, Denied, Expired, Cancelled }
```

The exact provider input remains on the execution machine. The relay stores a
bounded, tool-specific display payload plus a cryptographic digest of the exact
input. The response is valid only for that approval id and digest. Reshard never
turns "yes to command A" into permission for command B.

### Routing

- The requesting machine may open an approval only for an agent it owns and a
  chat where that agent is currently a member.
- The server derives `owner_id`; the machine cannot nominate an approver.
- Detailed approval events are delivered only to the owner user session.
- Other room members may see a redacted "waiting for owner approval" turn
  state, never the actionable card or sensitive command payload.
- Only the owner can resolve the request.
- Resolution is a compare-and-swap from `Pending`; the first valid terminal
  result wins.
- Invalid options, duplicate responses, wrong users, expired approvals, and
  approvals for cancelled runs are rejected server-side.
- Kicking or revoking an agent cancels its pending approvals immediately.

### State machine

```text
                    user allows
Running -> AwaitingApproval ----------> Running -> tool executes once
                |       |
                |       +-- user denies -------> Running with tool denial
                |       +-- timeout -----------> Running with tool denial
                |       +-- turn cancelled ----> Cancelled
                |       +-- process lost ------> Cancelled
                `---------- stale/digest mismatch -> Denied
```

The same chat does not begin another turn while one turn is waiting for an
approval. Other chats and agents continue normally.

### Approval card

The owner sees the card inline in the related conversation:

```text
Claude wants to run a command
Agent: claude-main      Project: reshard

  pnpm db:migrate --production

[ Deny ]                         [ Allow once ]
Expires in 4:32
```

Card rules:

- Exactly two initial actions: **Deny** and **Allow once**.
- No approval button receives automatic focus and Enter never means Allow.
- Show the agent, project, tool, target, and a tool-specific preview.
- Bash displays the exact command; file edits display path and bounded diff;
  network tools display host and method.
- Secrets, authorization headers, and large values are redacted before relay
  storage while the exact input remains local.
- The card visibly transitions to allowed, denied, expired, or cancelled and
  cannot be clicked again.
- "Always allow" is deferred. If added later, it is an explicit scoped policy
  editor, not a casual third button on a dangerous request.

---

## Permission policies

Initial policies:

| Policy | Meaning |
|---|---|
| `ask` | Default. Read-only discovery runs normally; state-changing or otherwise gated tools require an owner card. |
| `plan` | Read-only planning; writes and commands are denied. |
| `rules` | Explicit owner-managed allow/deny/ask rules, with Ask as fallback. |

An `unsafe-bypass` escape hatch may exist only as an explicit local CLI option
for an isolated disposable environment. It requires a typed confirmation,
cannot be enabled for a lent/shared agent, is shown persistently in the app,
and is never written by setup as a default. Fine-grained rules are the normal
way to reduce prompts.

---

## Protocol and storage work

Add typed commands and events rather than encoding approval state as chat text:

```rust
Command::RequestApproval { ...machine-bound fields... }
Command::ResolveApproval { approval, decision, input_digest }
Command::CancelApproval  { approval, reason }

Event::ApprovalRequested { approval }
Event::ApprovalResolved  { approval }
Event::ApprovalExpired   { approval }
```

Current storage is not yet a general event log, so v1 uses an `approvals` table
with transactional state transitions and an audit row for every terminal
decision. User-scoped event delivery is added alongside chat-scoped delivery;
approval details must not ride the current workspace-wide broadcast unchanged.

The supervisor holds a long-lived authenticated event connection. Polling may
exist as a recovery fallback, but the normal approval round trip is push-based.

---

## Implementation sequence

### Phase 1 — restore identity

- Add real register/login/refresh endpoints and password/session storage.
- Make `/bootstrap` development-only.
- Render `AuthPage` when Tauri is unauthenticated.
- Connect form fields to the real endpoints.
- Move the production Tauri token to the OS credential store.
- Add device authorization and `reshard auth` commands.

**Exit:** two separate users remain distinct across restart; logout revokes the
session; a CLI machine can be approved and revoked from the app.

### Phase 2 — shared discovery and interactive setup

**Implemented.** CLI and Tauri call the same `reshard_runtime::discover_local`
entry point. Runtime catalog loading, login-shell PATH recovery, probes, caching,
and report serialization therefore have one implementation and one schema.

- Add the shared runtime catalog crate.
- Implement login-shell PATH recovery, absolute-path resolution, versions,
  auth probes, capability probes, and JSON output.
- Add Claude, Codex, Gemini, Kimi, Hermes, OpenCode, and custom ACP definitions.
- Build `reshard setup`, `reshard runtimes`, and `reshard runtime doctor`.
- Report selected runtime inventory to the authenticated account.
- Show the same reports in Tauri.

**Exit:** CLI and Tauri return the same fixture reports; setup can identify
ready, logged-out, adapter-missing, invalid-config, and absent runtimes without
the user passing `--provider`.

### Phase 3 — approval vertical slice

**Implemented.**

- Add first-class approval types, persistence, authorization, expiry, and
  owner-scoped events.
- Convert the existing card visuals to the secure approval model.
- Add the local ApprovalBroker and a fake provider that requests one tool.
- Prove request -> owner card -> allow/deny -> provider continuation end to end.

**Exit:** wrong users and machines cannot view or resolve the request; timeout
denies; allow releases exactly one matching fake tool call.

### Phase 4 — Claude enforcement

**Implemented.**

- Implemented the private Reshard permission MCP server.
- Claude launches with `--permission-prompt-tool`, a controlled MCP config, default
  permission mode, and Reshard ask rules.
- The bypass flag remains absent; unsupported Claude versions stay disabled by
  the runtime capability gate.
- Feature probes and closed-failure diagnostics are implemented. On the
  currently installed Claude 2.1.226, `--permission-prompt-tool` is absent, so
  `reshard runtime doctor claude` correctly reports `unsupportedVersion` and
  refuses to enable it under Ask.
- Live subscription login, resume, allow, deny, expiry, cancellation, and
  broker-crash conformance remains the next verification step once a Claude
  build exposing the capability is installed.

**Exit:** no Claude state-changing tool can run through the default Reshard path
without a valid owner decision or explicit matching rule.

### Phase 5 — long-lived supervisor

- Replace per-message `sh -c` invocation with AgentWorker and ChatSession.
- Keep ACP adapters alive, reuse sessions, queue per chat, and run chats in
  parallel within provider limits.
- Add atomic checkpoints, cancellation, restart backoff, circuit breaking, and
  structured logs.
- Migrate existing `reshard.toml` agent records on first start.

**Exit:** sequential turns reuse a session and adapter; one waiting approval
does not block another chat; daemon restart preserves conversation continuity
without replaying a pending tool.

### Phase 6 — provider conformance and hardening

- Route native ACP permission requests through the same ApprovalBroker.
- Publish a conformance fixture for initialize, session, streaming, tool
  telemetry, permission, cancellation, and crash recovery.
- Mark runtimes capability-limited when they cannot meet the contract.
- Add push notification delivery for owner approvals.
- Add audit/export views and operational metrics.

---

## Test obligations

### Authentication

- Registration does not accept duplicate normalized email addresses.
- Password hashes and bearer tokens are never returned by diagnostic APIs.
- Expired, revoked, and wrong-scope sessions fail.
- Device codes are short-lived, single-use, rate-limited, and non-enumerable.

### Discovery

- GUI PATH and login-shell PATH resolve the same installed binary.
- A malicious frontend cannot make Tauri execute an undiscovered path.
- Every probe has a timeout and bounded output capture.
- One hung provider does not prevent other results from appearing.
- CLI JSON and Tauri reports conform to identical fixtures.

### Approvals

- Default setup never emits `--dangerously-skip-permissions`.
- User/project allow rules cannot override a Reshard-required ask rule.
- Only the server-derived owner receives the actionable request.
- A room member, another machine, or the requesting agent cannot approve it.
- Allow is bound to approval id, run id, tool call id, and exact input digest.
- Duplicate allow runs the tool at most once.
- Deny, timeout, disconnect, broker crash, daemon restart, kick, and cancellation
  all prevent execution.
- The card cannot approve after it becomes terminal.

### Supervisor

- Messages are serialized within one chat and concurrent across chats.
- Session state never crosses `(agent, chat)` keys.
- Backoff prevents crash loops.
- Queue limits produce visible backpressure instead of unbounded memory use.
- Restart never automatically replays a tool whose execution status is unknown.

---

## Expected file impact

- `core/src/lib.rs` — auth-facing protocol additions, runtime inventory,
  approval commands/events and state.
- `server/src/main.rs`, `server/src/store.rs` — real auth, device flow, scoped
  machine permissions, runtime inventory, approval persistence/routing.
- new `runtime/` crate — catalog, discovery, probes, capability model and
  shared fixtures.
- `cli/src/main.rs` — interactive setup, auth, runtimes, agents, service UX.
- `cli/src/up.rs` — progressively replaced by the supervisor.
- new CLI supervisor/approval/provider modules — worker pool, session queues,
  ApprovalBroker, permission MCP and migration.
- `cli/src/acp.rs` — persistent connections and ApprovalBroker integration.
- `cli/src/bin/claude-code-acp.rs` — Claude permission prompt tool and removal
  of default bypass.
- `apps/desktop/src/lib/auth.ts`, `use-chat.ts`, `App.tsx` — real auth and
  authenticated startup.
- `apps/desktop/src-tauri` — secure credential storage and shared discovery.
- `apps/desktop/src/components/chat/approval-card.tsx` — first-class,
  owner-scoped, terminal-state approval card.

---

## What is copied from Buzz—and what is not

Copy the design ideas:

- runtime catalog rather than provider conditionals
- login-shell PATH discovery
- explicit installed/adapter/auth/readiness states
- bounded parallel probes
- long-lived managed processes
- per-chat session reuse and queues
- provider capability metadata
- custom ACP harness definitions

Do not copy:

- Nostr identity and pairing
- desktop-only ownership of the runtime
- hand-written ACP protocol implementation
- API-key-only Claude assumptions
- unattended bypass behavior
- permission requests that are displayed but cannot enforce continuation
- their public/chat-level workflow approval stub

Reshard's intended advantage is a simpler authenticated control plane, a local
daemon that works equally on laptops and servers, Claude subscription support,
and a real owner-authorized approval round trip.

## Deferred

- "Always allow" and team-managed policies
- enterprise SSO and fleet policy
- remote installation of provider CLIs
- mobile push delivery beyond the approval event contract
- resuming an in-progress provider tool call after the local process is lost
- automatic provider selection by task
