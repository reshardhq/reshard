//! The rebeam protocol.
//!
//! Clients send [`Command`]s. The relay emits [`Event`]s. That is the entire
//! contract — every client (desktop app, CLI, future mobile) is an adapter
//! around these two types.
//!
//! Two rules define the system:
//!
//! 1. **Ephemeral by type.** [`Event::Status`] is fanned out but never
//!    persisted. Agent telemetry is high-frequency and worthless once seen.
//! 2. **Durable state is authoritative.** Chat content is appended with a
//!    per-chat monotonic `seq`; security objects such as approvals have their
//!    own transactional state machines and recovery endpoints.

use serde::{Deserialize, Serialize};

pub type Id = String;

/// Wall-clock milliseconds. The relay stamps these; clients never do.
pub type Millis = i64;

// ---------------------------------------------------------------------------
// Commands — client to relay
// ---------------------------------------------------------------------------

/// Everything a client can ask the relay to do.
///
/// Legacy author fields remain on the wire for adapters, but the relay derives
/// and overwrites them from the authenticated human or paired machine.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Command {
    /// Post a message to a chat.
    Send {
        chat: Id,
        author: Id,
        text: String,
        /// Client-generated; makes retries free.
        #[serde(default)]
        idem: Option<String>,
    },

    /// Ask a human to choose. Blocks the caller until resolved or expired.
    Ask {
        chat: Id,
        author: Id,
        text: String,
        options: Vec<String>,
    },

    /// Answer an open ask.
    Resolve { message: Id, option: String, by: Id },

    /// Report what an agent is doing right now. Never persisted.
    Status {
        chat: Id,
        author: Id,
        state: StatusState,
        #[serde(default)]
        label: Option<String>,
        /// Tool name, when `state` is [`StatusState::Tool`] — "Write", "Bash".
        #[serde(default)]
        tool: Option<String>,
        /// What the tool acted on — a path, a command, a URL.
        #[serde(default)]
        target: Option<String>,
    },

    /// Ask the owner of an agent to release one exact local tool invocation.
    /// The raw tool input never leaves the requesting machine; only its digest
    /// and a deliberately bounded display projection cross the relay.
    RequestApproval {
        agent: Id,
        chat: Id,
        run: Id,
        tool_call: Id,
        provider: String,
        tool: String,
        display: ApprovalDisplay,
        input_digest: String,
        expires_in_ms: Millis,
    },

    /// Resolve a pending tool approval. The digest makes stale or mismatched
    /// cards fail closed even when a client retries an old action.
    ResolveApproval {
        approval: Id,
        decision: ApprovalDecision,
        input_digest: String,
    },

    /// Withdraw a request when its provider turn is cancelled locally.
    CancelApproval { approval: Id, reason: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StatusState {
    Thinking,
    Tool,
    Done,
}

// ---------------------------------------------------------------------------
// Events — relay to clients
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "camelCase")]
pub enum Event {
    /// A durable message was appended to a chat.
    Message { message: Message },

    /// An existing message changed (today: an ask was resolved).
    MessageUpdated { message: Message },

    /// Chat directory metadata changed. The durable copy lives in `chats`;
    /// this frame keeps every open desktop window in sync.
    ChatUpdated { chat: Chat },

    /// Live agent telemetry. Broadcast, never stored.
    Status {
        chat: Id,
        author: Id,
        state: StatusState,
        label: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        tool: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        target: Option<String>,
    },

    /// Tell the paired gateway to discard one provider conversation. The
    /// shared room transcript remains available as fresh context.
    SessionReset { chat: Id, member: Id },

    /// Actionable approval details. The relay sends this only to the owning
    /// human and the exact machine that requested it, never to the room.
    ApprovalRequested { approval: Approval },

    /// A terminal approval transition (allowed, denied, or cancelled).
    ApprovalResolved { approval: Approval },

    /// Expiry is distinct so clients can explain why no action was released.
    ApprovalExpired { approval: Approval },
}

// ---------------------------------------------------------------------------
// Tool approvals — a separate security protocol, never a chat `Ask`
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalDisplay {
    /// Human-readable action summary, e.g. "Run database migration".
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ApprovalDecision {
    AllowOnce,
    Deny,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ApprovalState {
    Pending,
    Allowed,
    Denied,
    Expired,
    Cancelled,
}

impl ApprovalState {
    pub fn is_terminal(self) -> bool {
        self != Self::Pending
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Approval {
    pub id: Id,
    pub owner_id: Id,
    pub machine_id: Id,
    pub agent_id: Id,
    pub chat_id: Id,
    pub run_id: Id,
    pub tool_call_id: Id,
    pub provider: String,
    pub tool: String,
    pub display: ApprovalDisplay,
    pub input_digest: String,
    pub state: ApprovalState,
    pub expires_at: Millis,
    pub created_at: Millis,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_at: Option<Millis>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_by: Option<Id>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution_reason: Option<String>,
}

/// The durable unit. Field names match the client view model exactly, so the
/// generated TypeScript needs no translation layer.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub id: Id,
    pub channel_id: Id,
    pub author_id: Id,
    pub seq: i64,
    pub kind: MessageKind,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_option: Option<String>,
    pub created_at: Millis,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MessageKind {
    Text,
    Ask,
    /// Membership changes — "T31K added shopbot". Rendered, never replied to.
    System,
}

// ---------------------------------------------------------------------------
// Directory types — seeded for now, user-owned later
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Chat {
    pub id: Id,
    pub name: String,
    pub kind: ChatKind,
    pub member_ids: Vec<Id>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub topic: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avatar_seed: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ChatKind {
    Dm,
    Group,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Member {
    pub id: Id,
    pub name: String,
    #[serde(rename = "type")]
    pub kind: MemberKind,
    pub presence: Presence,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bio: Option<String>,
    /// Who this agent belongs to. Approvals route here, which is what makes an
    /// agent lendable to someone else's chat. `None` for humans.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_id: Option<Id>,
    /// The machine that redeemed the invite, so a code redeemed by the wrong
    /// box is visible rather than silent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MemberKind {
    Human,
    Agent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Presence {
    Online,
    Offline,
}

// ---------------------------------------------------------------------------
// Membership — an agent joins a chat exactly like a person does
// ---------------------------------------------------------------------------

/// An unredeemed capability. It does not know what will redeem it: a human
/// taps a link in the app, an agent runs `rebeam join`, and both produce the
/// same [`Membership`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Invite {
    /// Display form, `RB-7K2M-QX4P`.
    pub code: String,
    pub chat: Id,
    pub invited_by: Id,
    pub history: HistoryGrant,
    pub expires_at: Millis,
    /// Set once, at redemption. A second attempt is refused.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub redeemed_by: Option<Id>,
}

/// How far back a new member may read. Resolved to a seq floor at redemption.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "camelCase")]
pub enum HistoryGrant {
    /// The whole backlog — what you get when you add someone to a Slack channel.
    All,
    Since {
        ms: Millis,
    },
    /// Only what happens from here on.
    None,
}

impl Default for HistoryGrant {
    fn default() -> Self {
        Self::All
    }
}

/// One member's standing in one chat. Permissions, context, and revocation all
/// hang off this row.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Membership {
    pub chat: Id,
    pub member: Id,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub invited_by: Option<Id>,
    pub joined_at: Millis,
    /// The floor. Reads clamp to it server-side, so it is a boundary rather
    /// than a starting hint.
    pub history_floor_seq: i64,
    /// How far this member has actually read. Everything above it is unread,
    /// which is why joining needs no special code path — a new member simply
    /// has a lot of it.
    pub cursor_seq: i64,
    pub trigger: Trigger,
}

/// When a member wakes.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Trigger {
    /// On `@name`. A DM is an implicit, permanent mention.
    #[default]
    Mention,
    /// Every human message in the chat.
    All,
    /// Reads, never wakes — invited for context only.
    Never,
}

/// The 32 code characters, minus `0 O 1 I`. OpenClaw and Hermes landed on this
/// alphabet independently; it is the shape, not a preference.
pub const CODE_ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";

/// A member's name is also the thing you type after `@`, so it cannot contain
/// spaces — `@Hermes 1` has no end, and a member named that can never be
/// mentioned at all. Normalize to the kebab-case the seeded agents already
/// use: `"Hermes 1"` → `"hermes-1"`.
pub fn handle(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for c in raw.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').to_string()
}

/// Codes travel through clipboards, shell history, and other people's
/// terminals, so compare them leniently: case-insensitive, dashes optional,
/// with the `RB` prefix stripped.
pub fn normalize_code(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_uppercase())
        .collect();
    cleaned.strip_prefix("RB").unwrap_or(&cleaned).to_string()
}

impl Invite {
    pub const LEN: usize = 8;
    /// Long enough to walk to another machine, short enough that a leaked code
    /// is stale before it is useful.
    pub const TTL_MS: Millis = 60 * 60 * 1000;
    /// Per chat. A runaway backstop, not a workflow limit — inviting
    /// several agents to one group is the normal case.
    pub const MAX_PENDING: usize = 20;

    pub fn is_expired(&self, now: Millis) -> bool {
        now >= self.expires_at
    }

    pub fn is_open(&self, now: Millis) -> bool {
        self.redeemed_by.is_none() && !self.is_expired(now)
    }
}

impl Membership {
    /// Everything the member has not read yet, clamped to its floor. The window
    /// the bridge hands over on the next turn.
    pub fn unread(&self, head_seq: i64) -> (i64, i64) {
        (self.cursor_seq.max(self.history_floor_seq), head_seq)
    }
}

impl Event {
    /// Whether this event belongs in the log. Status telemetry does not.
    pub fn is_durable(&self) -> bool {
        !matches!(
            self,
            Event::Status { .. } | Event::SessionReset { .. } | Event::ChatUpdated { .. }
        )
    }

    pub fn chat(&self) -> &Id {
        match self {
            Event::Message { message } | Event::MessageUpdated { message } => &message.channel_id,
            Event::ChatUpdated { chat } => &chat.id,
            Event::Status { chat, .. } | Event::SessionReset { chat, .. } => chat,
            Event::ApprovalRequested { approval }
            | Event::ApprovalResolved { approval }
            | Event::ApprovalExpired { approval } => &approval.chat_id,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_events_are_never_durable() {
        let status = Event::Status {
            chat: "c1".into(),
            author: "a1".into(),
            state: StatusState::Thinking,
            label: None,
            tool: None,
            target: None,
        };
        assert!(!status.is_durable());
    }

    fn invite(redeemed: Option<&str>, expires_at: Millis) -> Invite {
        Invite {
            code: "7K2MQX4P".into(),
            chat: "g_warroom".into(),
            invited_by: "u_t31k".into(),
            history: HistoryGrant::All,
            expires_at,
            redeemed_by: redeemed.map(str::to_string),
        }
    }

    #[test]
    fn an_invite_is_single_use_and_expires() {
        assert!(invite(None, 1_000).is_open(0));
        assert!(!invite(None, 1_000).is_open(1_000), "expiry is inclusive");
        assert!(!invite(Some("a_shopbot"), 1_000).is_open(0));
    }

    #[test]
    fn codes_survive_the_trip_through_a_clipboard() {
        let canonical = normalize_code("RB-7K2M-QX4P");
        assert_eq!(canonical, "7K2MQX4P");
        for pasted in ["rb-7k2m-qx4p", "RB7K2MQX4P", "7K2M-QX4P", " 7k2mqx4p "] {
            assert_eq!(normalize_code(pasted), canonical, "{pasted:?}");
        }
    }

    #[test]
    fn a_name_with_a_space_can_never_be_mentioned_so_it_gets_kebabed() {
        assert_eq!(handle("Hermes 1"), "hermes-1");
        assert_eq!(handle("  Shop Bot  "), "shop-bot");
        assert_eq!(handle("Claude/Main"), "claude-main");
        assert_eq!(handle("kimi--research"), "kimi-research");
        // Already-good names survive untouched.
        assert_eq!(handle("codex-ci"), "codex-ci");
    }

    #[test]
    fn the_code_alphabet_excludes_the_ambiguous_glyphs() {
        assert_eq!(
            CODE_ALPHABET.len(),
            32,
            "must divide 256 evenly, or the
                    byte-to-char mapping skews toward early letters"
        );
        for c in b"0O1I" {
            assert!(!CODE_ALPHABET.contains(c), "{} is ambiguous", *c as char);
        }
    }

    fn membership(floor: i64, cursor: i64) -> Membership {
        Membership {
            chat: "g_warroom".into(),
            member: "a_shopbot".into(),
            invited_by: Some("u_t31k".into()),
            joined_at: 0,
            history_floor_seq: floor,
            cursor_seq: cursor,
            trigger: Trigger::Mention,
        }
    }

    #[test]
    fn a_full_history_grant_leaves_the_whole_backlog_unread() {
        // Joining is not a special path: the member simply has 40 unread.
        assert_eq!(membership(0, 0).unread(40), (0, 40));
    }

    #[test]
    fn a_none_grant_hides_everything_from_before_the_join() {
        assert_eq!(
            membership(40, 40).unread(40),
            (40, 40),
            "nothing to catch up on"
        );
        assert_eq!(
            membership(40, 40).unread(43),
            (40, 43),
            "only what came after"
        );
    }

    #[test]
    fn the_floor_outranks_a_cursor_that_points_below_it() {
        // A cursor rewound past the floor must not become a way to read the
        // history the invite withheld.
        assert_eq!(membership(40, 0).unread(50), (40, 50));
    }

    #[test]
    fn system_messages_round_trip() {
        let json = serde_json::to_string(&MessageKind::System).unwrap();
        assert_eq!(json, "\"system\"");
    }

    #[test]
    fn history_grants_are_tagged() {
        let json = serde_json::to_string(&HistoryGrant::Since { ms: 7 }).unwrap();
        assert_eq!(json, r#"{"t":"since","ms":7}"#);
        assert_eq!(
            serde_json::to_string(&HistoryGrant::default()).unwrap(),
            r#"{"t":"all"}"#
        );
    }

    #[test]
    fn commands_round_trip_as_tagged_json() {
        let json = r#"{"t":"send","chat":"c1","author":"a1","text":"hi"}"#;
        let cmd: Command = serde_json::from_str(json).unwrap();
        match cmd {
            Command::Send { chat, text, .. } => {
                assert_eq!(chat, "c1");
                assert_eq!(text, "hi");
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn approval_protocol_uses_distinct_typed_commands() {
        let command = Command::RequestApproval {
            agent: "a1".into(),
            chat: "c1".into(),
            run: "run_1".into(),
            tool_call: "call_1".into(),
            provider: "fake".into(),
            tool: "Write".into(),
            display: ApprovalDisplay {
                summary: "Write one file?".into(),
                project: Some("demo".into()),
                target: Some("README.md".into()),
                command: None,
            },
            input_digest: "a".repeat(64),
            expires_in_ms: 30_000,
        };
        let encoded = serde_json::to_value(&command).unwrap();
        assert_eq!(encoded["t"], "requestApproval");
        assert_eq!(encoded["toolCall"], "call_1");
        assert_eq!(encoded["display"]["target"], "README.md");
        assert!(serde_json::from_value::<Command>(encoded).is_ok());
        assert!(ApprovalState::Denied.is_terminal());
        assert!(!ApprovalState::Pending.is_terminal());
    }

    #[test]
    fn messages_serialize_in_client_shape() {
        let msg = Message {
            id: "m1".into(),
            channel_id: "c1".into(),
            author_id: "a1".into(),
            seq: 1,
            kind: MessageKind::Text,
            text: "hi".into(),
            options: None,
            resolved_option: None,
            created_at: 0,
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"channelId\""));
        assert!(json.contains("\"createdAt\""));
        assert!(!json.contains("options"));
    }
}
