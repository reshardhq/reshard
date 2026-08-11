import { useEffect, useRef, useState } from "react";
import { CheckIcon, SparklesIcon, XIcon } from "@/components/ui/icons";
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { mockBrain } from "@/lib/brain";
import { runPlan, type CommandCall } from "@/lib/commands";
import { useChat } from "@/lib/use-chat";
import { cn } from "@/lib/utils";

type StepState = "pending" | "running" | "done" | "error";

type CaddyTurn =
  | { role: "user"; text: string }
  | { role: "caddy"; text: string; plan?: CommandCall[]; steps?: StepState[] };

/**
 * caddy (⌘J) — palette-positioned chat. You talk, it plans through
 * the command bus and runs the steps inline. Brain is pluggable:
 * the panel only ever sees CommandCall[], so swapping the mock
 * for an LLM changes nothing here.
 */
export function CaddyPanel() {
  const { caddyOpen, setCaddyOpen } = useChat();
  const [turns, setTurns] = useState<CaddyTurn[]>([]);
  const [input, setInput] = useState("");
  const bottomRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: "instant" });
  }, [turns]);

  useEffect(() => {
    if (caddyOpen) setTimeout(() => inputRef.current?.focus(), 50);
  }, [caddyOpen]);

  const updateLast = (patch: Partial<Extract<CaddyTurn, { role: "caddy" }>>) => {
    setTurns((t) => {
      const next = [...t];
      const last = next[next.length - 1];
      if (last?.role === "caddy") next[next.length - 1] = { ...last, ...patch };
      return next;
    });
  };

  const submit = async () => {
    const text = input.trim();
    if (!text) return;
    setInput("");
    setTurns((t) => [...t, { role: "user", text }]);

    const plan = mockBrain.plan(text);
    if (!plan) {
      setTurns((t) => [
        ...t,
        {
          role: "caddy",
          text: 'Can\'t map that to actions yet. Try "open research and say hi" or "tell kimi-research to check the numbers".',
        },
      ]);
      return;
    }

    setTurns((t) => [
      ...t,
      {
        role: "caddy",
        text: plan.length === 1 ? "On it:" : `On it — ${plan.length} steps:`,
        plan,
        steps: plan.map(() => "pending" as StepState),
      },
    ]);

    const states: StepState[] = plan.map(() => "pending");
    try {
      await runPlan(plan, (i, phase, err) => {
        states[i] = phase;
        updateLast({ steps: [...states] });
        if (err) updateLast({ text: err });
      });
    } catch {
      return;
    }
    updateLast({ text: "Done." });
    // get out of the way — the result is in the app behind us
    setTimeout(() => {
      setCaddyOpen(false);
      setTurns([]);
    }, 650);
  };

  return (
    <Dialog open={caddyOpen} onOpenChange={setCaddyOpen}>
      <DialogContent
        className="top-1/3 max-w-lg translate-y-0 gap-0 overflow-hidden rounded-xl! p-0"
        showCloseButton={false}
      >
        <DialogHeader className="sr-only">
          <DialogTitle>caddy</DialogTitle>
          <DialogDescription>Ask caddy to do things for you</DialogDescription>
        </DialogHeader>

        {turns.length > 0 && (
          <div className="max-h-72 overflow-y-auto px-3 py-3">
            <div className="flex flex-col gap-2.5">
              {turns.map((turn, i) =>
                turn.role === "user" ? (
                  <div key={i} className="self-end">
                    <span className="inline-block rounded-lg bg-primary/10 px-2.5 py-1.5 text-[13px] text-foreground">
                      {turn.text}
                    </span>
                  </div>
                ) : (
                  <div key={i} className="flex gap-2 self-start">
                    <span className="mt-1 flex size-5 shrink-0 items-center justify-center rounded-md bg-violet-500/15 text-violet-400">
                      <SparklesIcon className="size-3" />
                    </span>
                    <div className="min-w-0">
                      <p className="text-[13px] leading-relaxed text-foreground/90">
                        {turn.text}
                      </p>
                      {turn.plan && (
                        <div className="mt-1.5 flex flex-col gap-1">
                          {turn.plan.map((call, j) => (
                            <div key={j} className="flex items-center gap-2">
                              {turn.steps?.[j] === "done" ? (
                                <CheckIcon className="size-3.5 shrink-0 text-green" />
                              ) : turn.steps?.[j] === "error" ? (
                                <XIcon className="size-3.5 shrink-0 text-red" />
                              ) : turn.steps?.[j] === "running" ? (
                                <span
                                  className="size-3 shrink-0 rounded-full border-[1.5px] border-line-strong border-t-foreground"
                                  style={{ animation: "spin 700ms linear infinite" }}
                                />
                              ) : (
                                <span className="size-3.5 shrink-0 rounded-full border border-muted-foreground/30" />
                              )}
                              <span
                                className={cn(
                                  "min-w-0 flex-1 truncate text-xs",
                                  turn.steps?.[j] === "done"
                                    ? "text-muted-foreground"
                                    : "text-foreground",
                                )}
                              >
                                {call.label}
                              </span>
                              <code className="shrink-0 font-mono text-[10px] text-muted-foreground/70">
                                {call.id}
                              </code>
                            </div>
                          ))}
                        </div>
                      )}
                    </div>
                  </div>
                ),
              )}
              <div ref={bottomRef} />
            </div>
          </div>
        )}

        <div
          className={cn(
            "flex items-center gap-2 px-3 py-2.5",
            turns.length > 0 && "border-t",
          )}
        >
          <SparklesIcon className="size-4 shrink-0 text-violet-400" />
          <Input
            ref={inputRef}
            value={input}
            onChange={(e) => setInput(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                e.preventDefault();
                void submit();
              }
            }}
            placeholder="Ask caddy — “open research and say hi”"
            className="h-8 border-0 bg-transparent text-[13px] shadow-none focus-visible:ring-0 dark:bg-transparent"
          />
          <kbd className="rounded-sm border border-border bg-muted/40 px-1 py-px font-mono text-[10px] text-muted-foreground">
            ⌘J
          </kbd>
        </div>
      </DialogContent>
    </Dialog>
  );
}
