import { useChat, store } from "./use-chat";

/**
 * The command bus: every app action, addressable by id + args.
 * UI surfaces (buttons, ⌘K agent, ⌘J caddy, context menus) are all
 * clients of this registry. The param schemas double as tool
 * definitions for LLM brains.
 */

export interface CommandDef {
  id: string;
  title: string;
  description: string;
  params: Record<string, { description: string; required?: boolean }>;
  run(args: Record<string, string>): Promise<string>;
}

export interface CommandCall {
  id: string;
  args: Record<string, string>;
  /** human-readable step label for plan previews */
  label: string;
}

function norm(s: string) {
  return s.toLowerCase().replace(/[^a-z0-9]/g, "");
}

export function resolveChannel(query: string) {
  const { channels } = useChat.getState();
  const q = norm(query);
  return (
    channels.find((c) => norm(c.name) === q) ??
    channels.find((c) => norm(c.name).includes(q) || q.includes(norm(c.name)))
  );
}

export function resolveMember(query: string) {
  const { members } = useChat.getState();
  const q = norm(query);
  return (
    members.find((m) => norm(m.name) === q) ??
    members.find((m) => norm(m.name).includes(q) || q.includes(norm(m.name)))
  );
}

export const COMMANDS: Record<string, CommandDef> = {
  "channel.open": {
    id: "channel.open",
    title: "Open channel",
    description: "Switch the active channel",
    params: { channel: { description: "Channel name", required: true } },
    async run({ channel }) {
      const target = resolveChannel(channel);
      if (!target) throw new Error(`No channel matching "${channel}"`);
      await useChat.getState().selectChannel(target.id);
      return `Opened #${target.name}`;
    },
  },
  "message.send": {
    id: "message.send",
    title: "Send message",
    description: "Send a message, optionally into a specific channel",
    params: {
      text: { description: "Message text", required: true },
      channel: { description: "Channel name (defaults to active)" },
    },
    async run({ text, channel }) {
      const state = useChat.getState();
      if (channel) {
        const target = resolveChannel(channel);
        if (!target) throw new Error(`No channel matching "${channel}"`);
        await state.selectChannel(target.id);
      }
      const channelId = useChat.getState().activeChannelId;
      if (!channelId) throw new Error("No active channel");
      await store.sendMessage(channelId, text);
      return `Sent "${text}"`;
    },
  },
  "member.mention": {
    id: "member.mention",
    title: "Mention member",
    description: "Insert an @mention into the composer",
    params: { name: { description: "Member name", required: true } },
    async run({ name }) {
      const member = resolveMember(name);
      if (!member) throw new Error(`No member matching "${name}"`);
      useChat.getState().setComposerInsert(`@${member.name} `);
      return `Mentioning @${member.name}`;
    },
  },
  "workspace.invite": {
    id: "workspace.invite",
    title: "Invite people or agents",
    description: "Open the invite dialog",
    params: {},
    async run() {
      useChat.getState().setInviteOpen(true);
      return "Opened invite";
    },
  },
};

export async function execute(call: CommandCall): Promise<string> {
  const def = COMMANDS[call.id];
  if (!def) throw new Error(`Unknown command: ${call.id}`);
  return def.run(call.args);
}

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

/** which UI control this call "presses", for the touch highlight */
function touchFor(call: CommandCall): string | null {
  switch (call.id) {
    case "channel.open": {
      const c = resolveChannel(call.args.channel);
      return c ? `channel:${c.id}` : null;
    }
    case "message.send":
      return "composer";
    case "member.mention": {
      const m = resolveMember(call.args.name);
      return m ? `member:${m.id}` : null;
    }
    case "workspace.invite":
      return "invite";
    default:
      return null;
  }
}

export type StepPhase = "running" | "done" | "error";

/**
 * Run a plan like an agent using the app: highlight the control,
 * beat, press, beat. All agent surfaces (⌘K, caddy) share this.
 */
export async function runPlan(
  plan: CommandCall[],
  onStep?: (index: number, phase: StepPhase, error?: string) => void,
): Promise<void> {
  const { setAgentTouch } = useChat.getState();
  for (let i = 0; i < plan.length; i++) {
    onStep?.(i, "running");
    setAgentTouch(touchFor(plan[i]));
    await sleep(520);
    try {
      await execute(plan[i]);
      onStep?.(i, "done");
    } catch (e) {
      setAgentTouch(null);
      onStep?.(i, "error", e instanceof Error ? e.message : String(e));
      throw e;
    }
    await sleep(220);
    setAgentTouch(null);
    await sleep(120);
  }
}
