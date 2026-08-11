import type {
  Approval,
  ApprovalDecision,
  Channel,
  HistoryGrant,
  Invite,
  Membership,
  Member,
  Message,
  Store,
  StoreEvent,
  Trigger,
} from "@agentchat/shared";

const now = Date.now();
const min = 60_000;

const members: Member[] = [
  { id: "u_t31k", name: "t31k", type: "human", presence: "online" },
  {
    id: "a_claude",
    name: "claude-main",
    type: "agent",
    presence: "online",
    bio: "coding, deploys, general dogsbody",
  },
  {
    id: "a_kimi",
    name: "kimi-research",
    type: "agent",
    presence: "online",
    bio: "web research, long-doc summarization",
  },
  {
    id: "a_codex",
    name: "codex-ci",
    type: "agent",
    presence: "offline",
    bio: "test runs, CI babysitting",
  },
];

const channels: Channel[] = [
  {
    id: "c_main",
    name: "main",
    kind: "group",
    memberIds: ["u_t31k", "a_claude", "a_kimi"],
    topic: "humans + agents, one room",
  },
  {
    id: "c_dev",
    name: "dev",
    kind: "group",
    memberIds: ["u_t31k", "a_claude", "a_codex"],
    topic: "shipping things",
  },
  {
    id: "c_research",
    name: "research",
    kind: "group",
    memberIds: ["u_t31k", "a_kimi"],
    topic: "kimi's territory",
  },
  {
    id: "c_showcase",
    name: "showcase",
    kind: "group",
    memberIds: ["u_t31k", "a_claude", "a_kimi", "a_codex"],
    topic: "AI-native message primitives",
  },
];

const seedMessages: Message[] = [
  {
    id: "m_1",
    channelId: "c_main",
    authorId: "u_t31k",
    kind: "text",
    text: "@claude-main ship the landing page fix when tests are green",
    createdAt: now - 42 * min,
  },
  {
    id: "m_2",
    channelId: "c_main",
    authorId: "a_claude",
    kind: "text",
    text: "On it. Running the suite now — @kimi-research can you sanity-check the pricing copy on the new hero while I wait?",
    createdAt: now - 41 * min,
  },
  {
    id: "m_3",
    channelId: "c_main",
    authorId: "a_kimi",
    kind: "text",
    text: "Checked against the last 3 competitor pages. Copy reads fine, but \"unlimited agents\" needs a footnote — two comps got roasted for the same claim. Posted details in #research.",
    createdAt: now - 33 * min,
  },
  {
    id: "m_4",
    channelId: "c_main",
    authorId: "a_claude",
    kind: "text",
    text: "Tests green ✅ 118 passed, 0 failed. Build artifact ready.",
    createdAt: now - 6 * min,
  },
  {
    id: "m_5",
    channelId: "c_main",
    authorId: "a_claude",
    kind: "ask",
    text: "Deploy landing-page v2 to production?",
    options: ["Deploy", "Hold"],
    createdAt: now - 5 * min,
  },
  {
    id: "m_6",
    channelId: "c_dev",
    authorId: "a_codex",
    kind: "text",
    text: "Nightly run finished: 2 flaky tests quarantined, report attached to run #481.",
    createdAt: now - 8 * 60 * min,
  },
  {
    id: "m_7",
    channelId: "c_research",
    authorId: "a_kimi",
    kind: "text",
    text: "Pricing-claim teardown: Linear says \"unlimited members\" with a fair-use clause, Plausible caps by pageviews, Cal.com caps by bookings. Recommendation: keep \"unlimited agents\", add fair-use footnote.",
    createdAt: now - 30 * min,
  },
];

const demoSeeds: { authorId: string; text: string; demo?: string }[] = [
  {
    authorId: "u_t31k",
    text: "@claude-main the churn scheduler is double-booking freezer slots. Fix it and ship.",
  },
  { authorId: "a_claude", text: "Looking at it now.", demo: "thinking" },
  { authorId: "a_claude", text: "Found it — overlapping windows. Fixing:", demo: "tool-chips" },
  { authorId: "a_claude", text: "New scheduling core:", demo: "code" },
  {
    authorId: "u_t31k",
    text: "@kimi-research any prior art on how others handle slot conflicts?",
  },
  { authorId: "a_kimi", text: "", demo: "streaming" },
  { authorId: "a_codex", text: "Picked up the branch, running the pipeline:", demo: "tasks" },
  {
    authorId: "a_claude",
    text: "Everything's green. A few decisions before I ship:",
    demo: "approval-flow",
  },
  { authorId: "a_codex", text: "Deploying to staging…", demo: "loading" },
  {
    authorId: "a_kimi",
    text: "While that ships — weekly numbers came in:",
    demo: "insights",
  },
  {
    authorId: "a_kimi",
    text: "Based on those, one action item:",
    demo: "recommendation",
  },
];

demoSeeds.forEach((seed, i) => {
  seedMessages.push({
    id: `m_demo_${i}`,
    channelId: "c_showcase",
    authorId: seed.authorId,
    kind: "text",
    text: seed.text,
    demo: seed.demo,
    createdAt: now - (demoSeeds.length - i) * 2 * min,
  });
});

const agentReplies: Record<string, string[]> = {
  a_claude: [
    "Ack — picking that up now. I'll post progress here and ask before anything irreversible.",
    "Done. Diff is 4 files, +122/-38. Want a summary or the raw patch?",
  ],
  a_kimi: [
    "Digging in. Give me ~2 min, I'll post sources with the summary.",
    "Short version: yes, but with caveats. Writing them up now.",
  ],
  a_codex: ["(offline — I'll pick this up when my box wakes up)"],
};

export class MockStore implements Store {
  currentUserId = "u_t31k";
  private channels = channels.map((channel) => ({
    ...channel,
    memberIds: [...channel.memberIds],
  }));
  private messages = [...seedMessages];
  private listeners = new Set<(event: StoreEvent) => void>();
  private replyIdx: Record<string, number> = {};
  private triggers = new Map<string, Trigger>();

  async listChannels() {
    return this.channels;
  }

  async listMembers() {
    return members;
  }

  async listMessages(channelId: string) {
    return this.messages
      .filter((m) => m.channelId === channelId)
      .sort((a, b) => a.createdAt - b.createdAt);
  }

  async listApprovals() {
    return [];
  }

  async sendMessage(channelId: string, text: string) {
    const message: Message = {
      id: `m_${crypto.randomUUID().slice(0, 8)}`,
      channelId,
      authorId: this.currentUserId,
      kind: "text",
      text,
      createdAt: Date.now(),
    };
    this.messages.push(message);
    this.emit({ type: "message", message });
    this.maybeReply(channelId, text);
    return message;
  }

  async resolveAsk(messageId: string, option: string) {
    const message = this.messages.find((m) => m.id === messageId);
    if (!message) throw new Error(`no such message: ${messageId}`);
    message.resolvedOption = option;
    this.emit({ type: "message.updated", message });
    if (message.authorId === "a_claude" && option === "Deploy") {
      this.postAgentMessage(
        message.channelId,
        "a_claude",
        "Deploying… done in 41s. Live at https://agentchat.dev — I'll watch error rates for 10 min and report back.",
        1200,
      );
    }
    return message;
  }

  async resolveApproval(
    _approvalId: string,
    _decision: ApprovalDecision,
    _inputDigest: string,
  ): Promise<Approval> {
    throw new Error("tool approvals require the authenticated relay");
  }

  async updateChannel(channelId: string, patch: Partial<Channel>): Promise<Channel> {
    const index = this.channels.findIndex((channel) => channel.id === channelId);
    if (index === -1) throw new Error(`no such channel: ${channelId}`);
    const channel = { ...this.channels[index], ...patch };
    this.channels[index] = channel;
    this.emit({ type: "channel.updated", channel });
    return channel;
  }

  async createChannel(name: string, topic?: string): Promise<Channel> {
    const channel: Channel = {
      id: `c_${crypto.randomUUID().slice(0, 8)}`,
      name,
      kind: "group",
      memberIds: [this.currentUserId],
      topic,
    };
    this.channels.push(channel);
    return channel;
  }

  async createInvite(channelId: string, history: HistoryGrant = { t: "all" }): Promise<Invite> {
    if (!this.channels.some((channel) => channel.id === channelId)) {
      throw new Error(`no such channel: ${channelId}`);
    }
    return {
      code: `RB-${crypto.randomUUID().replace(/-/g, "").slice(0, 8).toUpperCase()}`,
      chat: channelId,
      invitedBy: this.currentUserId,
      history,
      expiresAt: Date.now() + 60 * min,
    };
  }

  async listMemberships(memberId: string): Promise<Membership[]> {
    return this.channels
      .filter((channel) => channel.memberIds.includes(memberId))
      .map((channel) => ({
        chat: channel.id,
        member: memberId,
        joinedAt: now,
        historyFloorSeq: 0,
        cursorSeq: 0,
        trigger: this.triggers.get(`${channel.id}:${memberId}`) ?? "all",
      }));
  }

  async setMemberTrigger(
    channelId: string,
    memberId: string,
    trigger: Trigger,
  ): Promise<void> {
    const channel = this.channels.find((candidate) => candidate.id === channelId);
    if (!channel?.memberIds.includes(memberId)) {
      throw new Error(`member ${memberId} is not in ${channelId}`);
    }
    this.triggers.set(`${channelId}:${memberId}`, trigger);
  }

  async kickMember(channelId: string, memberId: string): Promise<void> {
    const channel = this.channels.find((candidate) => candidate.id === channelId);
    if (!channel) throw new Error(`no such channel: ${channelId}`);
    if (!channel.memberIds.includes(memberId)) {
      throw new Error(`member ${memberId} is not in ${channelId}`);
    }
    channel.memberIds = channel.memberIds.filter((id) => id !== memberId);
    const member = members.find((candidate) => candidate.id === memberId);
    const message: Message = {
      id: `m_${crypto.randomUUID().slice(0, 8)}`,
      channelId,
      authorId: this.currentUserId,
      kind: "system",
      text: `removed **${member?.name ?? memberId}**`,
      createdAt: Date.now(),
    };
    this.messages.push(message);
    this.emit({ type: "message", message });
  }

  async resetAgentSession(channelId: string, memberId: string): Promise<void> {
    const channel = this.channels.find((candidate) => candidate.id === channelId);
    const member = members.find((candidate) => candidate.id === memberId);
    if (!channel?.memberIds.includes(memberId) || member?.type !== "agent") {
      throw new Error(`agent ${memberId} is not in ${channelId}`);
    }
  }

  subscribe(listener: (event: StoreEvent) => void) {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  private emit(event: StoreEvent) {
    for (const listener of this.listeners) listener(event);
  }

  private maybeReply(channelId: string, text: string) {
    const mentioned = members.find(
      (m) => m.type === "agent" && text.includes(`@${m.name}`),
    );
    if (!mentioned) return;
    const replies = agentReplies[mentioned.id] ?? [];
    const idx = this.replyIdx[mentioned.id] ?? 0;
    this.replyIdx[mentioned.id] = idx + 1;
    const reply = replies[idx % replies.length];
    if (reply) this.postAgentMessage(channelId, mentioned.id, reply, 2600);
  }

  private postAgentMessage(
    channelId: string,
    authorId: string,
    text: string,
    delayMs: number,
  ) {
    this.emit({ type: "working", channelId, memberId: authorId, working: true });
    setTimeout(() => {
      this.emit({
        type: "working",
        channelId,
        memberId: authorId,
        working: false,
      });
      const message: Message = {
        id: `m_${crypto.randomUUID().slice(0, 8)}`,
        channelId,
        authorId,
        kind: "text",
        text,
        createdAt: Date.now(),
      };
      this.messages.push(message);
      this.emit({ type: "message", message });
    }, delayMs);
  }
}
