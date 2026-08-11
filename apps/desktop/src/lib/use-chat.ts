import { create } from "zustand";
import type {
  Approval,
  ApprovalDecision,
  Channel,
  Member,
  HistoryGrant,
  Message,
  Store,
  TurnItem,
} from "@agentchat/shared";
import { MockStore } from "./mock-store";
import { notifyApprovalRequested } from "./notify";
import { LiveStore } from "./live-store";
import * as auth from "./auth";
import type { AuthUser } from "./auth";

/**
 * The app talks to one Store. Which one is a transport decision made once at
 * boot: the relay if it answers, the in-memory store otherwise. There is no
 * mock branch anywhere else in the codebase.
 */
export let store: Store = new MockStore();

interface ChatState {
  channels: Channel[];
  members: Member[];
  messages: Message[];
  approvals: Approval[];
  activeChannelId: string | null;
  inviteOpen: boolean;
  newChatOpen: boolean;
  caddyOpen: boolean;
  prefsOpen: boolean;
  rightSidebarOpen: boolean;
  activityOpen: boolean;
  authed: boolean;
  authReady: boolean;
  user: AuthUser | null;
  machineCount: number;
  machineOnline: number;
  /** text the command palette wants inserted into the composer */
  composerInsert: string | null;
  /** agents currently composing, per channel */
  working: { channelId: string; memberId: string }[];
  /** UI control the in-app agent is currently "pressing" */
  agentTouch: string | null;
  /** unread message count per conversation */
  unread: Record<string, number>;
  /** divider position captured when opening a chat with unread */
  unreadMarker: { channelId: string; messageId: string; count: number } | null;
  /** which transport the store ended up on */
  relay: "mock" | "live";
  /** live agent turns in flight, one per chat */
  turns: Record<string, { memberId: string; startedAt: number; items: TurnItem[] }>;
  init(): Promise<void>;
  initAuth(): Promise<void>;
  refreshMachines(): Promise<void>;
  signIn(email: string, password: string): Promise<void>;
  register(name: string, email: string, password: string): Promise<void>;
  claimUsername(username: string, displayName: string): Promise<void>;
  signOut(): Promise<void>;
  selectChannel(channelId: string): Promise<void>;
  send(text: string): Promise<void>;
  resolveAsk(messageId: string, option: string): Promise<void>;
  resolveApproval(approvalId: string, decision: ApprovalDecision): Promise<void>;
  setInviteOpen(open: boolean): void;
  setNewChatOpen(open: boolean): void;
  createChannel(name: string, topic?: string, agentId?: string): Promise<Channel>;
  kickMember(channelId: string, memberId: string): Promise<void>;
  resetAgentSession(channelId: string, memberId: string): Promise<void>;
  setCaddyOpen(open: boolean): void;
  setPrefsOpen(open: boolean): void;
  setRightSidebarOpen(open: boolean): void;
  setActivityOpen(open: boolean): void;
  setAuthed(authed: boolean): void;
  updateChannel(
    channelId: string,
    patch: Partial<Pick<Channel, "name" | "topic" | "avatarSeed" | "memberIds">>,
  ): Promise<void>;
  setComposerInsert(text: string | null): void;
  setAgentTouch(key: string | null): void;
  reorderChannels(activeId: string, overId: string): void;
}

export const useChat = create<ChatState>((set, get) => ({
  channels: [],
  members: [],
  messages: [],
  approvals: [],
  activeChannelId: null,
  inviteOpen: false,
  newChatOpen: false,
  caddyOpen: false,
  prefsOpen: false,
  rightSidebarOpen: false,
  activityOpen: false,
  authed: false,
  authReady: false,
  user: null,
  machineCount: 0,
  machineOnline: 0,
  composerInsert: null,
  working: [],
  agentTouch: null,
  unread: { g_warroom: 2, c_dm_claude: 1, c_dm_codex: 1 },
  unreadMarker: null,
  relay: "mock",
  turns: {},

  async initAuth() {
    const user = await auth.currentUser().catch(() => null);
    const machines = user
      ? await auth.machineStatus().catch(() => ({ count: 0, online: 0, machines: [] }))
      : { count: 0, online: 0, machines: [] };
    set({
      user,
      machineCount: machines.count,
      machineOnline: machines.online,
      authed: user != null,
      authReady: true,
    });
  },

  async refreshMachines() {
    const machines = await auth.machineStatus().catch(() => ({ count: 0, online: 0, machines: [] }));
    set({ machineCount: machines.count, machineOnline: machines.online });
  },

  async signIn(email, password) {
    const user = await auth.login(email, password);
    set({ user, authed: true });
  },

  async register(name, email, password) {
    const user = await auth.register(name, email, password);
    set({ user, authed: true });
  },
  async claimUsername(username, displayName) {
    const user = await auth.claimUsername(username, displayName);
    set({ user, authed: true });
  },

  async signOut() {
    if (store instanceof LiveStore) store.close();
    await auth.logout();
    store = new MockStore();
    set({ user: null, authed: false, channels: [], messages: [], approvals: [], activeChannelId: null });
  },

  async init() {
    // StrictMode double-invokes effects; never subscribe twice
    if (get().channels.length) return;

    // Pick the transport once: the relay if it's up, memory otherwise.
    try {
      store = await LiveStore.connect();
      set({ relay: "live", unread: {} });
    } catch {
      set({ relay: "mock" });
    }

    const [channels, members, approvals] = await Promise.all([
      store.listChannels(),
      store.listMembers(),
      store.listApprovals(),
    ]);
    // restore user's saved channel order
    try {
      const saved: string[] = JSON.parse(
        localStorage.getItem("agentchat.channelOrder") ?? "[]",
      );
      channels.sort((a, b) => {
        const ai = saved.indexOf(a.id);
        const bi = saved.indexOf(b.id);
        return (ai === -1 ? 999 : ai) - (bi === -1 ? 999 : bi);
      });
    } catch {
      // ignore corrupt saved order
    }
    set({ channels, members, approvals });
    store.subscribe((event) => {
      const { activeChannelId, messages } = get();
      if (event.type === "message") {
        // A join or leave changes the directory, and the relay tells us about
        // it only through the system line — without this the member list and
        // the header count silently stay at whatever they were at boot.
        if (event.message.kind === "system") {
          void Promise.all([store.listChannels(), store.listMembers()]).then(
            ([channels, members]) => set({ channels, members }),
          );
        }
        // Content landing closes the turn — Hermes needs a __reset__ signal
        // for this because it edits a borrowed message bubble; we just drop
        // the turn, because the trace is our own object.
        const turns = { ...get().turns };
        if (turns[event.message.channelId]?.memberId === event.message.authorId) {
          delete turns[event.message.channelId];
        }
        set({
          turns,
          working: get().working.filter(
            (w) =>
              !(
                w.channelId === event.message.channelId &&
                w.memberId === event.message.authorId
              ),
          ),
        });
        if (event.message.channelId === activeChannelId) {
          if (!messages.some((m) => m.id === event.message.id)) {
            set({ messages: [...messages, event.message] });
          }
        } else {
          const unread = { ...get().unread };
          unread[event.message.channelId] =
            (unread[event.message.channelId] ?? 0) + 1;
          set({ unread });
        }
      }
      if (event.type === "message.updated") {
        set({
          messages: get().messages.map((m) =>
            m.id === event.message.id ? { ...event.message } : m,
          ),
        });
      }
      if (event.type === "turn") {
        const turns = { ...get().turns };
        if (!event.item) {
          delete turns[event.channelId];
          set({ turns });
          return;
        }
        const current = turns[event.channelId];
        const open =
          current?.memberId === event.memberId
            ? current
            : { memberId: event.memberId, startedAt: Date.now(), items: [] };

        // Identical consecutive lines collapse into a counter rather than
        // stacking — stolen from Hermes, worth stealing.
        const last = open.items[open.items.length - 1];
        const same =
          last &&
          last.kind === event.item.kind &&
          last.tool === event.item.tool &&
          last.target === event.item.target &&
          last.label === event.item.label;

        const items = same
          ? [...open.items.slice(0, -1), { ...last, count: last.count + 1 }]
          : [...open.items, { ...event.item, count: 1, at: Date.now() }];

        turns[event.channelId] = { ...open, items };
        set({ turns });
      }
      if (event.type === "working") {
        const rest = get().working.filter(
          (w) =>
            !(w.channelId === event.channelId && w.memberId === event.memberId),
        );
        set({
          working: event.working
            ? [...rest, { channelId: event.channelId, memberId: event.memberId }]
            : rest,
        });
      }
      if (event.type === "channel.updated") {
        set({
          channels: get().channels.map((c) =>
            c.id === event.channel.id ? { ...event.channel } : c,
          ),
        });
      }
      if (event.type === "presence") {
        set({
          members: get().members.map((m) =>
            m.id === event.memberId ? { ...m, presence: event.presence } : m,
          ),
        });
      }
      if (
        event.type === "approval.requested" ||
        event.type === "approval.resolved" ||
        event.type === "approval.expired"
      ) {
        const approvals = get().approvals;
        const index = approvals.findIndex(
          (approval) => approval.id === event.approval.id,
        );
        set({
          approvals:
            index === -1
              ? [...approvals, event.approval]
              : approvals.map((approval) =>
                  approval.id === event.approval.id ? event.approval : approval,
                ),
        });
        // Surface a native notification the first time we see a pending
        // approval, so the owner doesn't miss the card when unfocused.
        if (
          event.type === "approval.requested" &&
          index === -1 &&
          event.approval.state === "pending"
        ) {
          void notifyApprovalRequested(event.approval);
        }
      }
    });
    const first = channels[0];
    if (first) await get().selectChannel(first.id);
  },

  async selectChannel(channelId) {
    const count = get().unread[channelId] ?? 0;
    const unread = { ...get().unread };
    delete unread[channelId];
    set({ activeChannelId: channelId, unread });
    const messages = await store.listMessages(channelId);
    const firstUnread =
      count > 0 ? messages[Math.max(0, messages.length - count)] : undefined;
    set({
      messages,
      unreadMarker: firstUnread
        ? { channelId, messageId: firstUnread.id, count }
        : null,
    });
  },

  async send(text) {
    const { activeChannelId } = get();
    if (!activeChannelId || !text.trim()) return;
    await store.sendMessage(activeChannelId, text.trim());
  },

  async resolveAsk(messageId, option) {
    await store.resolveAsk(messageId, option);
  },

  async resolveApproval(approvalId, decision) {
    const approval = get().approvals.find((item) => item.id === approvalId);
    if (!approval || approval.state !== "pending") return;
    const updated = await store.resolveApproval(
      approval.id,
      decision,
      approval.inputDigest,
    );
    set({
      approvals: get().approvals.map((item) =>
        item.id === updated.id ? updated : item,
      ),
    });
  },

  setInviteOpen(open) {
    set({ inviteOpen: open });
  },

  setNewChatOpen(open) {
    set({ newChatOpen: open });
  },

  async createChannel(name, topic, agentId) {
    const channel = await store.createChannel(name, topic, agentId);
    set({ channels: [...get().channels, channel] });
    await get().selectChannel(channel.id);
    return channel;
  },

  async kickMember(channelId, memberId) {
    await store.kickMember(channelId, memberId);
    // The relay posts a system line; refetching keeps the member list honest
    // whichever store is behind us.
    set({ channels: await store.listChannels() });
  },

  async resetAgentSession(channelId, memberId) {
    await store.resetAgentSession(channelId, memberId);
  },

  setCaddyOpen(open) {
    set({ caddyOpen: open });
  },

  setPrefsOpen(open) {
    set({ prefsOpen: open });
  },

  setRightSidebarOpen(open) {
    set({ rightSidebarOpen: open });
  },

  setActivityOpen(open) {
    set({ activityOpen: open });
  },

  setAuthed(authed) {
    set({ authed });
  },

  async updateChannel(channelId, patch) {
    const channel = await store.updateChannel(channelId, patch);
    set({
      channels: get().channels.map((candidate) =>
        candidate.id === channel.id ? channel : candidate,
      ),
    });
  },

  setComposerInsert(text) {
    set({ composerInsert: text });
  },

  setAgentTouch(key) {
    set({ agentTouch: key });
  },

  reorderChannels(activeId, overId) {
    const channels = [...get().channels];
    const from = channels.findIndex((c) => c.id === activeId);
    const to = channels.findIndex((c) => c.id === overId);
    if (from === -1 || to === -1 || from === to) return;
    const [moved] = channels.splice(from, 1);
    channels.splice(to, 0, moved);
    set({ channels });
    localStorage.setItem(
      "agentchat.channelOrder",
      JSON.stringify(channels.map((c) => c.id)),
    );
  },
}));

export function memberById(members: Member[], id: string): Member | undefined {
  return members.find((m) => m.id === id);
}

export async function createInvite(channelId: string, history?: HistoryGrant) {
  return store.createInvite(channelId, history);
}
