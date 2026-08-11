import {
  type CommandCall,
  resolveChannel,
  resolveMember,
} from "./commands";

/**
 * A brain turns natural language into a plan of command calls.
 * MockBrain is deterministic pattern-matching so the demo works
 * offline; an LLM brain (Claude tool-calling over COMMANDS) plugs
 * in behind this same interface later.
 */
export interface Brain {
  plan(input: string): CommandCall[] | null;
}

export const mockBrain: Brain = {
  plan(input) {
    const text = input.trim();
    if (text.length < 3) return null;

    // "invite an agent / someone"
    if (/^invite\b/i.test(text)) {
      return [{ id: "workspace.invite", args: {}, label: "Open invite" }];
    }

    // "tell kimi to check the numbers" / "say hi to claude"
    const tell = text.match(/^(?:tell|ask)\s+([\w\s-]+?)\s+(?:to\s+)?(.+)$/i);
    if (tell) {
      const member = resolveMember(tell[1]);
      if (member) {
        return [
          {
            id: "message.send",
            args: { text: `@${member.name} ${tell[2]}` },
            label: `Message @${member.name}: "${tell[2]}"`,
          },
        ];
      }
    }

    // "send hi to kimi" / "send kimi a message saying hi"
    const send =
      text.match(/^send\s+(.+?)\s+to\s+([\w\s-]+)$/i) ??
      text.match(/^send\s+([\w\s-]+?)\s+a message\s+(?:saying\s+)?(.+)$/i);
    if (send) {
      const [a, b] = [send[1], send[2]];
      const member = resolveMember(b) ?? resolveMember(a);
      const msg = resolveMember(b) ? a : b;
      if (member) {
        return [
          {
            id: "message.send",
            args: { text: `@${member.name} ${msg}` },
            label: `Message @${member.name}: "${msg}"`,
          },
        ];
      }
    }

    // "open research" / "open up kimi research and say hi"
    const open = text.match(
      /^open(?:\s+up)?\s+([\w\s#-]+?)(?:\s+and\s+(?:say|send)(?:\s+it)?(?:\s+a\s+message)?(?:\s+saying)?\s+(.+))?$/i,
    );
    if (open) {
      const target = open[1];
      const message = open[2];
      const member = resolveMember(target);
      const channel = resolveChannel(target);
      const plan: CommandCall[] = [];
      if (channel) {
        plan.push({
          id: "channel.open",
          args: { channel: channel.name },
          label: `Open #${channel.name}`,
        });
      }
      if (message) {
        const textOut = member ? `@${member.name} ${message}` : message;
        plan.push({
          id: "message.send",
          args: { text: textOut },
          label: member
            ? `Message @${member.name}: "${message}"`
            : `Send "${message}"`,
        });
      } else if (!channel && member) {
        plan.push({
          id: "member.mention",
          args: { name: member.name },
          label: `Mention @${member.name}`,
        });
      }
      if (plan.length) return plan;
    }

    return null;
  },
};
