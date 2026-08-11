import { useChat } from "@/lib/use-chat";
import { cn } from "@/lib/utils";

function Key({ children }: { children: React.ReactNode }) {
  return (
    <kbd className="inline-flex size-5 shrink-0 items-center justify-center rounded-sm border border-border bg-muted/40 font-mono text-[10px] leading-none text-foreground/70">
      {children}
    </kbd>
  );
}

function Hint({ keys, label }: { keys: string[]; label: string }) {
  return (
    <span className="flex items-center gap-1">
      <span className="flex items-center gap-0.5">
        {keys.map((key, index) => (
          <Key key={`${key}-${index}`}>{key}</Key>
        ))}
      </span>
      <span>{label}</span>
    </span>
  );
}

export function StatusBar() {
  const { channels, activeChannelId, activityOpen, setActivityOpen, relay } = useChat();
  const idx = channels.findIndex((c) => c.id === activeChannelId);

  return (
    <footer className="fixed inset-x-0 bottom-0 z-30 flex h-8 items-center gap-4 border-t bg-sidebar px-3 font-mono text-[11px] text-muted-foreground">
      <button
        className="flex items-center gap-1.5 rounded-sm px-1 transition-colors hover:bg-muted/40 hover:text-foreground"
        onClick={() => setActivityOpen(!activityOpen)}
      >
        <span
          className={cn(
            "size-1.5 rounded-full",
            relay === "live" ? "bg-emerald-400" : "bg-amber-400",
          )}
        />
        {relay === "live" ? "connected" : "offline"}
      </button>
      <div className="ml-auto flex items-center gap-4">
        <Hint keys={["⌘", "K"]} label="agent" />
        <Hint keys={["⌘", "J"]} label="caddy" />
        <Hint keys={["⌘", "\\"]} label="left" />
        {idx >= 0 && (
          <span className="text-foreground/60">
            {channels[idx].name} [{idx + 1}/{channels.length}]
          </span>
        )}
      </div>
    </footer>
  );
}
