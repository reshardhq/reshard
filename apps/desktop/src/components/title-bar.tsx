import { useChat } from "@/lib/use-chat";

export function TitleBar() {
  const { channels, activeChannelId, members } = useChat();
  const channel = channels.find((c) => c.id === activeChannelId);
  const online = members.filter(
    (m) => m.type === "agent" && m.presence === "online",
  ).length;

  return (
    <header
      data-tauri-drag-region
      className="fixed inset-x-0 top-0 z-30 flex h-9.5 items-center border-b bg-sidebar px-3"
    >
      {/* clearance for macOS traffic lights */}
      <div className="w-16 shrink-0" data-tauri-drag-region />
      <div
        data-tauri-drag-region
        className="pointer-events-none absolute inset-x-0 flex justify-center"
      >
        <span className="font-mono text-[11px] tracking-tight text-muted-foreground">
          agentchat{channel ? ` — ${channel.name}` : ""}
        </span>
      </div>
      <span
        data-tauri-drag-region
        className="ml-auto font-mono text-[11px] text-muted-foreground"
      >
        {online} agent{online === 1 ? "" : "s"} online
      </span>
    </header>
  );
}
