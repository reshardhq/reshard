import { useEffect, useState } from "react";
import { ThinkingState } from "@/components/ai";
import { MemberAvatar } from "@/components/member-avatar";
import { useChat, memberById } from "@/lib/use-chat";

/**
 * An agent's turn while it is happening, rendered with the same
 * `ThinkingState` trace the showcase uses — now driven by real relay events
 * instead of a scripted timeline.
 *
 * Hermes streams this telemetry into Telegram by editing one message bubble,
 * which costs it throttled edits, length splitting, dedup hacks, a cleanup
 * pass, and per-platform capability checks. Owning the client turns all of
 * that into rendering an array.
 */
export function LiveTurn() {
  const { turns, activeChannelId, members } = useChat();
  const turn = activeChannelId ? turns[activeChannelId] : undefined;
  const elapsed = useElapsed(turn?.startedAt);

  if (!turn) return null;
  const member = memberById(members, turn.memberId);
  const thinking = [...turn.items].reverse().find((i) => i.kind === "thinking");

  return (
    <div className="mx-4 mb-1.5 flex gap-2.5">
      {member && <MemberAvatar member={member} className="mt-0.5 size-5" />}
      <ThinkingState
        variant="Coding"
        live={{
          working: true,
          label: thinking?.label ?? `${member?.name ?? "agent"} is working`,
          doneLabel: `Worked for ${elapsed}s`,
          rows: turn.items.map((item) =>
            item.kind === "tool"
              ? {
                  primary: item.tool ?? "tool",
                  secondary:
                    item.count > 1
                      ? `${item.target ?? ""} ×${item.count}`
                      : item.target,
                  mono: true,
                }
              : { primary: item.label ?? "thinking" },
          ),
        }}
      />
    </div>
  );
}

function useElapsed(startedAt?: number) {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!startedAt) return;
    const t = setInterval(() => setNow(Date.now()), 500);
    return () => clearInterval(t);
  }, [startedAt]);
  return startedAt ? Math.max(0, Math.round((now - startedAt) / 1000)) : 0;
}
