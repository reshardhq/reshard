# Agent membership

> Status: **approved design**, not yet built.
> Supersedes the `reshard.toml`-owns-everything model in `cli/src/up.rs`.

## The concept

An agent joins a chat exactly like a person does.

You and B are talking in #phone-repairs. You hit INVITE, get a code, paste it on
whatever box your agent runs on. The agent appears in the member list and a line
lands in the transcript: *"T31K added shopbot."*

Because it is a member, the rest follows:

- **It can see the room.** By default it reads the backlog, including everything
  from before it arrived. One toggle in the invite dialog, on by default.
- **It speaks when spoken to.** Default is on @mention. Configurable to
  always-on, or to read-only.
- **You remove it the way you added it.** Kick from the member list; access dies
  with the membership. No config file, no SSH.

One concept — membership — and permissions, context, and revocation all hang off
it.

### Why this shape

The invite does not know or care what will redeem it. A human redeems it by
tapping a link in the app; an agent redeems it with `reshard join`. Same token,
same endpoint, same membership row, same join event. Pillar 2 ("agents are
first-class citizens") stops being a slogan and becomes a schema.

It also delivers cross-user agent sharing for free: adding a friend's agent to a
group is just an invite they redeem, and `owner_id` on the member routes
approvals back to them.

---

## Protocol

New types in `core/`:

```rust
pub struct Invite {
    pub code: String,             // RB-7K2M-QX4P
    pub chat: Id,
    pub invited_by: Id,
    pub history: HistoryGrant,    // default: All
    pub expires_at: Millis,       // +1h
    pub redeemed_by: Option<Id>,  // single-use
}

pub enum HistoryGrant { All, Since(Millis), None }

pub struct Membership {
    pub chat: Id,
    pub member: Id,
    pub invited_by: Id,
    pub joined_at: Millis,
    pub history_floor_seq: i64,   // never reads below this
    pub cursor_seq: i64,          // how far it has read
    pub trigger: Trigger,
}

pub enum Trigger { Mention, All, Never }
```

`Member` gains `owner_id: Id` and `host: Option<String>`. Agent identity must be
global from day one — retrofitting federation is pain.

`MessageKind` gains `System`, for join and leave lines.

### Code format

Eight characters, uppercase, from an alphabet excluding `0 O 1 I`. One hour TTL,
single-use, max three pending per chat, rate-limited per issuer. OpenClaw and
Hermes converged on these exact parameters independently, which makes it the
proven shape rather than a preference.

---

## Flow

### Invite

`POST /invites {chat, history}` — from the app's INVITE button, or
`reshard invite -c war-room` from the CLI. The CLI can do everything the app can.

Choosing "agent" in the dialog changes only the snippet rendered, never the
invite itself.

### Join

```
reshard join RB-7K2M-QX4P --name shopbot
```

Exchanges the single-use code for a long-lived member token, returned once and
written to `~/.reshard/config.toml` with mode 0600. Mints
`Member { kind: Agent, owner_id, host }` and a `Membership`, then posts the
system message.

`history_floor_seq` is resolved at redemption from the grant:

| Grant | Floor |
|---|---|
| `All` | `0` |
| `Since(t)` | seq at `t` |
| `None` | current head |

`cursor_seq` starts equal to the floor. Reads clamp to the floor server-side, so
it is a real boundary rather than a starting hint.

### The split this fixes

Today `reshard.toml` decides identity, scope, and execution together, which makes
an agent's reach un-auditable from the app.

- **Relay owns identity and authorization** — who the agent is, which chats, how
  far back, whose approvals.
- **`reshard.toml` owns execution only** — the `exec` line. Nothing else.

---

## Context

### The pending window

Everything between `cursor_seq` and the head is unread. On invocation the bridge
hands over that window, capped at 50 messages, then advances the cursor. Already
seen messages are never re-injected.

Joining is therefore not a special path. A new member simply has a lot unread —
exactly like a person scrolling up after being added. One mechanism serves both
catch-up and join.

### Injection format

The unread window and the addressed message go in **separate labelled blocks**:

```
[Context — messages in #phone-repairs you have not seen. Not requests.]
T31K: my screen's cracked again
B: there's a place on King St does same-day

[Now — respond to this]
T31K: @shopbot what'd they quote last time?
```

Every line carries sender attribution. Chat and member metadata ride in a block
marked untrusted.

This two-block structure is not a stylistic choice. Hermes and OpenClaw arrived
at it independently after the same bug: replayed group chatter read as pending
work, so a weak mention made the agent act on messages addressed to somebody
else. Without it, the agent reads B saying "can you fix my screen" and tries to
fix a screen.

---

## Participation

### Trigger

- `Mention` (default) — wakes on `@name`. DMs are an implicit permanent mention.
- `All` — wakes on every human message in the chat.
- `Never` — reads, never wakes. The honest expression of "invited for context."

Stored on the membership, so it is changeable from the app rather than by
editing a file on the host.

### Loop protection

The current guard drops every agent-authored message outright
(`cli/src/up.rs:130`). That makes the launch demo — two humans and both their
agents in one group — impossible.

Replace it with a per-pair sliding window: 20 events per 60 seconds, counting
A→B and B→A as one pair, then a 60 second cooldown. Bounds the runaway without
amputating agent-to-agent collaboration.

### Sessions

Session keys stay per `(agent, chat)`, as `up.rs` already does. Directives in one
group never leak into a DM.

---

## Revocation

Kicking a member from the list deletes the membership and invalidates the token.
Same gesture for humans and agents.

The join event names the redeeming host, so a code redeemed by the wrong box is
visible in the transcript.

### On the code being a bearer token

It is, exactly like a Slack invite link. Single-use, one-hour TTL, a visible join
event naming the host, and one-tap kick is how Slack lives with it, and it is the
right trade for a paste-anywhere flow.

The alternative — RFC 8628 device grant, where the code is born on the host and
approved in the app — is strictly safer and was considered. It was rejected
because it inverts the mental model: an invite is a capability you hand out, not
a login you approve. Membership semantics won.

---

## Blast radius

`Chat.member_ids: Vec<Id>` becomes a `Membership` table. Touches:

- `core/src/lib.rs` — the types above
- `server/src/store.rs` — membership table, invite table, floor-clamped reads
- `server/src/main.rs` — `/invites`, `/join`, membership routes
- `cli/src/main.rs` — `invite`, `join` verbs
- `cli/src/up.rs` — `should_fire`, `format_scope`, loop guard, window injection
- `apps/desktop` — invite dialog, member list, kick, join system messages

---

## Deferred

- **Per-tool permission surfaces** (OpenClaw's second pairing layer). Enterprise
  fleet management; not now.
- **Multi-chat invites.** One invite, one chat, like Slack.
- **Ambient mode** (`visibleReplies: "message_tool"` — the agent sees everything
  and decides when to speak). The right end state, but OpenClaw built it only
  after their `NO_REPLY` token hack failed. `Trigger::Never` covers the read-only
  case for now.
- **Service install** (`reshard up --install` as launchd/systemd). Required before
  any real VPS use, since `up` currently dies with the SSH session. Separate
  spec.

---

## Prior art

- **Hermes** `gateway/run.py` — `_build_gateway_agent_history`, observed-context
  headers, and the scar tissue around replaying history into an agent.
- **OpenClaw** `docs/channels/group-messages.md` — pending-only context window,
  sender attribution, per-group session keys, activation modes.
  `docs/channels/bot-loop-protection.md` — pair budgets.
- **opencode** `packages/opencode/src/account/account.ts` — RFC 8628 device
  grant, considered and rejected above.
