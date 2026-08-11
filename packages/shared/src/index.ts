export type MemberType = "human" | "agent";
export type Presence = "online" | "offline";

export interface Member {
  id: string;
  name: string;
  type: MemberType;
  presence: Presence;
  /** short capability blurb, mostly for agents */
  bio?: string;
}

export type ConversationKind = "dm" | "group";

/** A conversation: a 1:1 DM or a created group. (Type keeps the
 * legacy `Channel` name to avoid a repo-wide rename for now.) */
export interface Channel {
  id: string;
  /** group name; for DMs, the other member's name */
  name: string;
  kind: ConversationKind;
  memberIds: string[];
  topic?: string;
  /** custom dither-avatar seed; falls back to name */
  avatarSeed?: string;
}

/** `system` is the transcript narrating itself — joins, leaves, renames. */
export type MessageKind = "text" | "ask" | "system";

export interface Message {
  id: string;
  channelId: string;
  authorId: string;
  kind: MessageKind;
  text: string;
  /** ask messages only */
  options?: string[];
  /** set once a human taps a button */
  resolvedOption?: string;
  /** renders a named AI primitive instead of text (showcase/demo) */
  demo?: string;
  createdAt: number;
}

export type ApprovalState =
  | "pending"
  | "allowed"
  | "denied"
  | "expired"
  | "cancelled";

export type ApprovalDecision = "allowOnce" | "deny";

export interface ApprovalDisplay {
  summary: string;
  project?: string;
  target?: string;
  command?: string;
}

/** A bounded relay projection of one exact machine-local tool invocation. */
export interface Approval {
  id: string;
  ownerId: string;
  machineId: string;
  agentId: string;
  chatId: string;
  runId: string;
  toolCallId: string;
  provider: string;
  tool: string;
  display: ApprovalDisplay;
  inputDigest: string;
  state: ApprovalState;
  expiresAt: number;
  createdAt: number;
  resolvedAt?: number;
  resolvedBy?: string;
  resolutionReason?: string;
}

/** How far back a new member may read. Resolved to a seq floor on redemption. */
export type HistoryGrant =
  | { t: "all" }
  | { t: "none" }
  | { t: "since"; ms: number };

/**
 * An unredeemed capability. It does not know what will redeem it: a human taps
 * a link, an agent runs `rebeam join`, and both produce the same membership.
 */
export interface Invite {
  code: string;
  chat: string;
  invitedBy: string;
  history: HistoryGrant;
  expiresAt: number;
}

/** Controls which messages wake an agent inside a chat. */
export type Trigger = "mention" | "all" | "never";

/** One member's per-chat delivery settings. */
export interface Membership {
  chat: string;
  member: string;
  invitedBy?: string;
  joinedAt: number;
  historyFloorSeq: number;
  cursorSeq: number;
  trigger: Trigger;
}

/** One line of live agent telemetry inside a turn. */
export interface TurnItem {
  kind: "thinking" | "tool";
  label?: string;
  /** tool name — Write, Bash, Read */
  tool?: string;
  /** what the tool acted on */
  target?: string;
  /** repeats collapse instead of stacking (×3) */
  count: number;
  at: number;
}

export type StoreEvent =
  | { type: "message"; message: Message }
  | { type: "message.updated"; message: Message }
  | { type: "presence"; memberId: string; presence: Presence }
  | { type: "working"; channelId: string; memberId: string; working: boolean }
  | {
      type: "turn";
      channelId: string;
      memberId: string;
      /** null closes the turn */
      item: Omit<TurnItem, "count" | "at"> | null;
    }
  | { type: "channel.updated"; channel: Channel }
  | { type: "approval.requested"; approval: Approval }
  | { type: "approval.resolved"; approval: Approval }
  | { type: "approval.expired"; approval: Approval };

/**
 * The only surface the desktop app talks to. Phase 1 backs it with an
 * in-memory mock; phase 2 swaps in the relay client without UI changes.
 */
export interface Store {
  currentUserId: string;
  listChannels(): Promise<Channel[]>;
  listMembers(): Promise<Member[]>;
  listMessages(channelId: string): Promise<Message[]>;
  /** Owner-scoped recovery after reconnect. Other room members never see these. */
  listApprovals(): Promise<Approval[]>;
  sendMessage(channelId: string, text: string): Promise<Message>;
  resolveAsk(messageId: string, option: string): Promise<Message>;
  resolveApproval(
    approvalId: string,
    decision: ApprovalDecision,
    inputDigest: string,
  ): Promise<Approval>;
  updateChannel(
    channelId: string,
    patch: Partial<Pick<Channel, "name" | "topic" | "avatarSeed" | "memberIds">>,
  ): Promise<Channel>;
  createChannel(name: string, topic?: string, agentId?: string): Promise<Channel>;
  /** Mint an invite to one chat. Whoever redeems it becomes a member of it. */
  createInvite(channelId: string, history?: HistoryGrant): Promise<Invite>;
  listMemberships(memberId: string): Promise<Membership[]>;
  setMemberTrigger(
    channelId: string,
    memberId: string,
    trigger: Trigger,
  ): Promise<void>;
  /** Remove a member. The same gesture for humans and agents, and the only
   * revocation there is — the token dies with the membership. */
  kickMember(channelId: string, memberId: string): Promise<void>;
  /** Discard one agent's private provider session while preserving the room. */
  resetAgentSession(channelId: string, memberId: string): Promise<void>;
  subscribe(listener: (event: StoreEvent) => void): () => void;
}
