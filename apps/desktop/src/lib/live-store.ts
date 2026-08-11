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
import {
  authenticatedUser,
  authorizedFetch,
  sessionToken,
} from "./auth";

const RELAY = import.meta.env.VITE_RELAY ?? "http://127.0.0.1:8787";

/** Wire events from the relay — mirrors rebeam-core::Event. */
type WireEvent =
  | { t: "message"; message: Message }
  | { t: "messageUpdated"; message: Message }
  | { t: "chatUpdated"; chat: Channel }
  | {
      t: "status";
      chat: string;
      author: string;
      state: "thinking" | "tool" | "done";
      label?: string | null;
      tool?: string | null;
      target?: string | null;
    }
  | { t: "approvalRequested"; approval: Approval }
  | { t: "approvalResolved"; approval: Approval }
  | { t: "approvalExpired"; approval: Approval };

/**
 * The same Store interface the UI already talks to, backed by the relay.
 * Commands go over HTTP; events arrive on one WebSocket.
 */
export class LiveStore implements Store {
  currentUserId: string;
  private listeners = new Set<(event: StoreEvent) => void>();
  private socket?: WebSocket;
  private stopped = false;

  constructor(private relay = RELAY) {
    const current = authenticatedUser();
    if (!current) throw new Error("LiveStore requires an authenticated user");
    this.currentUserId = current.id;
  }

  /** Resolves only if the relay answers — the caller falls back to the mock. */
  static async connect(relay = RELAY, timeoutMs = 800): Promise<LiveStore> {
    const abort = new AbortController();
    const timer = setTimeout(() => abort.abort(), timeoutMs);
    try {
      const res = await fetch(`${relay}/health`, { signal: abort.signal });
      if (!res.ok) throw new Error(`relay unhealthy: ${res.status}`);
      const store = new LiveStore(relay);
      store.open();
      return store;
    } finally {
      clearTimeout(timer);
    }
  }

  async listChannels() {
    return this.get<Channel[]>("/chats");
  }

  async listMembers() {
    return this.get<Member[]>("/members");
  }

  async listMessages(channelId: string) {
    return this.get<Message[]>(`/chats/${channelId}/messages`);
  }

  async listApprovals() {
    return this.get<Approval[]>("/approvals");
  }

  async sendMessage(channelId: string, text: string) {
    const event = await this.command({
      t: "send",
      chat: channelId,
      author: this.currentUserId,
      text,
    });
    return (event as { message: Message }).message;
  }

  async resolveAsk(messageId: string, option: string) {
    const event = await this.command({
      t: "resolve",
      message: messageId,
      option,
      by: this.currentUserId,
    });
    return (event as { message: Message }).message;
  }

  async resolveApproval(
    approvalId: string,
    decision: ApprovalDecision,
    inputDigest: string,
  ) {
    const event = await this.command({
      t: "resolveApproval",
      approval: approvalId,
      decision,
      inputDigest,
    });
    return (event as { approval: Approval }).approval;
  }

  async updateChannel(
    channelId: string,
    patch: Partial<Pick<Channel, "name" | "topic" | "avatarSeed" | "memberIds">>,
  ): Promise<Channel> {
    const body: Record<string, unknown> = {};
    if ("name" in patch) body.name = patch.name;
    if ("topic" in patch) body.topic = patch.topic ?? "";
    if ("avatarSeed" in patch) body.avatarSeed = patch.avatarSeed ?? "";
    const res = await authorizedFetch(`${this.relay}/chats/${channelId}`, {
      method: "PATCH",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(body),
    });
    if (!res.ok) throw new Error(await errorText(res));
    return res.json() as Promise<Channel>;
  }

  async createChannel(name: string, topic?: string, agentId?: string) {
    return this.post<Channel>("/chats", { name, topic: topic || null, agentId: agentId || null });
  }

  async createInvite(channelId: string, history: HistoryGrant = { t: "all" }) {
    return this.post<Invite>("/invites", { chat: channelId, history });
  }

  async listMemberships(memberId: string) {
    return this.get<Membership[]>(`/members/${memberId}/memberships`);
  }

  async setMemberTrigger(
    channelId: string,
    memberId: string,
    trigger: Trigger,
  ) {
    await this.post(`/chats/${channelId}/members/${memberId}`, { trigger });
  }

  async kickMember(channelId: string, memberId: string) {
    const res = await authorizedFetch(
      `${this.relay}/chats/${channelId}/members/${memberId}`,
      { method: "DELETE" },
    );
    if (!res.ok) throw new Error(await errorText(res));
  }

  async resetAgentSession(channelId: string, memberId: string) {
    await this.post(`/chats/${channelId}/members/${memberId}/reset-session`, {});
  }

  subscribe(listener: (event: StoreEvent) => void) {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  close() {
    this.stopped = true;
    const socket = this.socket;
    this.socket = undefined;
    socket?.close(1000, "signed out");
    this.listeners.clear();
  }

  // -- transport ----------------------------------------------------------

  private open() {
    if (this.stopped) return;
    const token = sessionToken();
    if (!token) return;
    const url = this.relay.replace(/^http/, "ws") + `/stream?token=${encodeURIComponent(token)}`;
    const socket = new WebSocket(url);
    this.socket = socket;

    socket.onmessage = (raw) => {
      let wire: WireEvent;
      try {
        wire = JSON.parse(raw.data as string);
      } catch {
        return;
      }
      this.emit(wire);
    };

    // Refresh first: the relay deliberately closes sockets whose access token
    // expires or is revoked.
    socket.onclose = () => {
      if (this.stopped || this.socket !== socket) return;
      setTimeout(() => {
        void authorizedFetch(`${this.relay}/auth/me`).then((response) => {
          if (response.ok && !this.stopped && this.socket === socket) this.open();
        });
      }, 1000);
    };
  }

  private emit(wire: WireEvent) {
    const event = toStoreEvent(wire);
    if (!event) return;
    for (const listener of this.listeners) listener(event);
  }

  private async get<T>(path: string): Promise<T> {
    const res = await authorizedFetch(this.relay + path);
    if (!res.ok) throw new Error(`${path} → ${res.status}`);
    return res.json() as Promise<T>;
  }

  private async post<T>(path: string, body: unknown): Promise<T> {
    const res = await authorizedFetch(this.relay + path, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(body),
    });
    if (!res.ok) throw new Error(await errorText(res));
    return res.json() as Promise<T>;
  }

  private async command(cmd: Record<string, unknown>) {
    const res = await authorizedFetch(`${this.relay}/commands`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(cmd),
    });
    if (!res.ok) throw new Error(`command failed: ${await res.text()}`);
    return res.json();
  }

}

/**
 * The relay's refusal, which is the useful half of a failed invite — "that
 * invite has expired" beats "400".
 */
async function errorText(res: Response) {
  const body = await res.text();
  try {
    return (JSON.parse(body) as { error?: string }).error ?? body;
  } catch {
    return body || `request failed: ${res.status}`;
  }
}

/** Wire event → the shape the app's store already understands. */
function toStoreEvent(wire: WireEvent): StoreEvent | null {
  switch (wire.t) {
    case "message":
      return { type: "message", message: wire.message };
    case "messageUpdated":
      return { type: "message.updated", message: wire.message };
    case "chatUpdated":
      return { type: "channel.updated", channel: wire.chat };
    case "status":
      return {
        type: "turn",
        channelId: wire.chat,
        memberId: wire.author,
        item:
          wire.state === "done"
            ? null
            : {
                kind: wire.state,
                label: wire.label ?? undefined,
                tool: wire.tool ?? undefined,
                target: wire.target ?? undefined,
              },
      };
    case "approvalRequested":
      return { type: "approval.requested", approval: wire.approval };
    case "approvalResolved":
      return { type: "approval.resolved", approval: wire.approval };
    case "approvalExpired":
      return { type: "approval.expired", approval: wire.approval };
    default:
      return null;
  }
}
