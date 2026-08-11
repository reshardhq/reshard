import { useEffect } from "react";
import { AreaChart } from "@/components/dither-kit/area-chart";
import { Area } from "@/components/dither-kit/area";
import { XAxis } from "@/components/dither-kit/x-axis";
import { Tooltip } from "@/components/dither-kit/tooltip";
import { Sparkline } from "@/components/dither-kit/sparkline";
import type { DitherColor } from "@/components/dither-kit/palette";
import { AvatarFrame } from "@/components/member-avatar";
import { LoadingState } from "@/components/ai";
import { useChat } from "@/lib/use-chat";
import { cn } from "@/lib/utils";

const relayData = [
  { hour: "10a", messages: 14 },
  { hour: "11a", messages: 32 },
  { hour: "12p", messages: 21 },
  { hour: "1p", messages: 44 },
  { hour: "2p", messages: 61 },
  { hour: "3p", messages: 38 },
  { hour: "4p", messages: 57 },
];

const relayConfig = {
  messages: { label: "Messages", color: "green" },
} as const;

const AGENT_SPARKS: Record<string, { data: number[]; color: DitherColor }> = {
  "claude-main": { data: [4, 9, 6, 12, 18, 11, 16], color: "orange" },
  "kimi-research": { data: [2, 5, 9, 4, 8, 13, 7], color: "blue" },
  "codex-ci": { data: [7, 3, 0, 0, 2, 6, 1], color: "green" },
};

/**
 * In-pane bottom sheet: lives inside the chat pane (not a portal),
 * so it never covers the sidebars. Backdrop + slide are clipped by
 * the pane's overflow.
 */
export function ActivitySheet() {
  const { activityOpen, setActivityOpen, members, channels, working } =
    useChat();
  const agents = members.filter((m) => m.type === "agent");

  useEffect(() => {
    if (!activityOpen) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setActivityOpen(false);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [activityOpen, setActivityOpen]);

  return (
    <>
      <div
        aria-hidden
        onClick={() => setActivityOpen(false)}
        className={cn(
          "absolute inset-0 z-40 bg-black/20 backdrop-blur-[1px] transition-opacity duration-300",
          activityOpen ? "opacity-100" : "pointer-events-none opacity-0",
        )}
      />
      <div
        role="dialog"
        aria-label="Activity"
        className={cn(
          "absolute inset-x-0 bottom-0 z-50 border-t bg-popover text-popover-foreground shadow-lg transition-transform duration-300 ease-[cubic-bezier(0.32,0.72,0,1)]",
          activityOpen ? "translate-y-0" : "translate-y-full",
        )}
      >
        <div className="px-4 pt-3">
          <p className="text-sm font-semibold">Activity</p>
          <p className="text-xs text-muted-foreground">
            Relay connection and what your agents are up to.
          </p>
        </div>
        <div className="grid gap-4 p-4 pb-6 sm:grid-cols-2">
          <div className="rounded-lg border p-3">
            <div className="flex items-baseline justify-between">
              <p className="font-mono text-[11px] uppercase tracking-wide text-muted-foreground">
                Relay · today
              </p>
              <p className="flex items-center gap-1.5 text-xs">
                <span className="size-1.5 rounded-full bg-emerald-400" />
                connected · mock
              </p>
            </div>
            <div className="mt-2 h-32">
              <AreaChart data={relayData} config={relayConfig} bloom="low">
                <XAxis dataKey="hour" />
                <Tooltip labelKey="hour" />
                <Area dataKey="messages" variant="dotted" />
              </AreaChart>
            </div>
            <p className="mt-1.5 text-xs text-muted-foreground">
              {channels.length} chats · {members.length} members
            </p>
          </div>
          <div className="rounded-lg border p-3">
            <p className="font-mono text-[11px] uppercase tracking-wide text-muted-foreground">
              Agents · messages this week
            </p>
            <div className="mt-2.5 flex flex-col gap-2.5">
              {agents.map((agent) => {
                const busy = working.find((w) => w.memberId === agent.id);
                const spark = AGENT_SPARKS[agent.name];
                return (
                  <div key={agent.id} className="flex items-center gap-2.5">
                    <AvatarFrame name={agent.name} className="size-5" />
                    <span className="w-28 truncate text-sm">{agent.name}</span>
                    {spark && (
                      <div className="h-6 min-w-0 flex-1">
                        <Sparkline
                          data={spark.data}
                          color={spark.color}
                          variant="gradient"
                          bloomOnHover
                        />
                      </div>
                    )}
                    {busy ? (
                      <LoadingState label="working" variant="Dots" />
                    ) : (
                      <span className="font-mono text-[10px] uppercase text-muted-foreground">
                        {agent.presence}
                      </span>
                    )}
                  </div>
                );
              })}
            </div>
          </div>
        </div>
      </div>
    </>
  );
}
