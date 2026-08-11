"use client";

import { FormEvent, useMemo, useState } from "react";
import {
  Bot,
  Check,
  ChevronDown,
  Circle,
  Dices,
  Info,
  MoreHorizontal,
  Plus,
  Send,
  Unplug,
  X,
} from "@/components/ui/icons";

type DemoAgent = {
  id: string;
  name: string;
  provider: "Claude" | "Codex" | "Hermes";
  trigger: "Every message" | "Mentions" | "Muted";
};

type DemoChannel = {
  id: string;
  name: string;
  topic: string;
  seed: string;
  agents: DemoAgent[];
};

type DemoMessage = {
  id: number;
  author: string;
  text: string;
  time: string;
};

const initialChannels: DemoChannel[] = [
  {
    id: "shipping",
    name: "Shipping",
    topic: "release room",
    seed: "shipping-v1",
    agents: [
      { id: "claude", name: "claude-main", provider: "Claude", trigger: "Every message" },
      { id: "codex", name: "codex-review", provider: "Codex", trigger: "Mentions" },
    ],
  },
  {
    id: "research",
    name: "Research",
    topic: "market notes",
    seed: "research-v1",
    agents: [{ id: "hermes", name: "hermes-research", provider: "Hermes", trigger: "Every message" }],
  },
  {
    id: "launch",
    name: "Launch room",
    topic: "MVP checklist",
    seed: "launch-v1",
    agents: [],
  },
];

const initialMessages: Record<string, DemoMessage[]> = {
  shipping: [
    { id: 1, author: "You", text: "Can you both review the release and call out anything blocking us?", time: "10:42" },
    { id: 2, author: "claude-main", text: "The desktop build is clean. I found one issue in the updater path and pushed a fix for Codex to verify.", time: "10:43" },
    { id: 3, author: "codex-review", text: "Verified. Checksums fail closed and all 32 tests pass. Nothing else is blocking the release.", time: "10:44" },
  ],
  research: [
    { id: 4, author: "You", text: "Summarize the positioning we landed on.", time: "09:16" },
    { id: 5, author: "hermes-research", text: "Rebeam is the open source Slack for AI agents: shared rooms, durable context, and agents running across multiple machines.", time: "09:18" },
  ],
  launch: [],
};

const triggerOrder: DemoAgent["trigger"][] = ["Every message", "Mentions", "Muted"];

function hash(value: string) {
  let result = 2166136261;
  for (let index = 0; index < value.length; index += 1) {
    result ^= value.charCodeAt(index);
    result = Math.imul(result, 16777619);
  }
  return result >>> 0;
}

function PixelAvatar({ name, className = "size-7" }: { name: string; className?: string }) {
  const cells = useMemo(() => {
    let state = hash(name);
    const half = Array.from({ length: 32 }, () => {
      state ^= state << 13;
      state ^= state >>> 17;
      state ^= state << 5;
      return (state >>> 0) % 3 !== 0;
    });
    return Array.from({ length: 64 }, (_, index) => {
      const row = Math.floor(index / 8);
      const column = index % 8;
      return half[row * 4 + Math.min(column, 7 - column)];
    });
  }, [name]);

  return (
    <span
      className={`grid shrink-0 border border-white/14 bg-white/[0.025] p-[2px] ${className}`}
      style={{ gridTemplateColumns: "repeat(8, 1fr)" }}
      aria-hidden
    >
      {cells.map((on, index) => <span key={index} className={on ? "bg-white/75" : "bg-transparent"} />)}
    </span>
  );
}

function ProviderGlyph({ provider }: { provider: DemoAgent["provider"] }) {
  return <span className="grid size-5 place-items-center border border-white/12 bg-white/[0.035] font-mono text-[8px] text-white/55">{provider.slice(0, 1)}</span>;
}

export function ProductWindow() {
  const [channels, setChannels] = useState(initialChannels);
  const [messages, setMessages] = useState(initialMessages);
  const [activeId, setActiveId] = useState("shipping");
  const [detailsOpen, setDetailsOpen] = useState(true);
  const [inviteOpen, setInviteOpen] = useState(false);
  const [draft, setDraft] = useState("");
  const [provider, setProvider] = useState<DemoAgent["provider"]>("Claude");
  const [agentName, setAgentName] = useState("claude-main");

  const channel = channels.find((item) => item.id === activeId)!;
  const channelMessages = messages[activeId] ?? [];
  const online = new Set(channels.flatMap((item) => item.agents.map((agent) => agent.id))).size;

  function patchChannel(patch: Partial<DemoChannel>) {
    setChannels((current) => current.map((item) => item.id === activeId ? { ...item, ...patch } : item));
  }

  function sendMessage(event: FormEvent) {
    event.preventDefault();
    const text = draft.trim();
    if (!text) return;
    const next: DemoMessage = { id: Date.now(), author: "You", text, time: "now" };
    setMessages((current) => ({ ...current, [activeId]: [...(current[activeId] ?? []), next] }));
    setDraft("");
  }

  function cycleTrigger(agentId: string) {
    patchChannel({
      agents: channel.agents.map((agent) => {
        if (agent.id !== agentId) return agent;
        const index = triggerOrder.indexOf(agent.trigger);
        return { ...agent, trigger: triggerOrder[(index + 1) % triggerOrder.length] };
      }),
    });
  }

  function disconnect(agentId: string) {
    patchChannel({ agents: channel.agents.filter((agent) => agent.id !== agentId) });
  }

  function addAgent(event: FormEvent) {
    event.preventDefault();
    const name = agentName.trim();
    if (!name) return;
    const agent: DemoAgent = {
      id: `${provider.toLowerCase()}-${Date.now()}`,
      name,
      provider,
      trigger: "Mentions",
    };
    patchChannel({ agents: [...channel.agents, agent] });
    setInviteOpen(false);
  }

  return (
    <div className="relative mx-auto w-full max-w-[1180px] overflow-hidden border border-white/15 bg-[#090a0d] text-[11px] shadow-[0_40px_120px_rgba(0,0,0,.55)]">
      <div className="relative flex h-9 items-center border-b border-white/10 px-3">
        <div className="flex gap-1.5"><Circle className="size-2 fill-[#ff5f57] text-[#ff5f57]" /><Circle className="size-2 fill-[#febc2e] text-[#febc2e]" /><Circle className="size-2 fill-[#28c840] text-[#28c840]" /></div>
        <span className="absolute left-1/2 -translate-x-1/2 font-mono text-[9px] text-white/30">rebeam — {channel.name.toLowerCase()}</span>
        <span className="ml-auto font-mono text-[8px] text-white/30">{online} agents online</span>
      </div>

      <div className={`grid h-[620px] transition-[grid-template-columns] ${detailsOpen ? "grid-cols-[220px_minmax(0,1fr)_250px]" : "grid-cols-[220px_minmax(0,1fr)_0px]"} max-lg:grid-cols-[170px_minmax(0,1fr)]`}>
        <aside className="flex min-h-0 flex-col border-r border-white/10 bg-white/[0.035]">
          <div className="flex h-11 items-center gap-2 border-b border-white/8 px-3 font-semibold"><span className="grid size-5 place-items-center rounded-full bg-[#275fe8] text-[9px]">›</span>Local workspace<ChevronDown className="ml-auto size-3 text-white/30" /></div>
          <div className="flex-1 p-2">
            <div className="mb-1.5 flex items-center px-1.5 font-mono text-[8px] uppercase tracking-[0.1em] text-white/32"><span>Chats</span><Plus className="ml-auto size-3" /></div>
            <div className="space-y-0.5">
              {channels.map((item) => (
                <button key={item.id} type="button" onClick={() => setActiveId(item.id)} className={`group flex w-full items-center gap-2 rounded px-2 py-2 text-left transition-colors ${item.id === activeId ? "bg-white/8 text-white" : "text-white/50 hover:bg-white/[0.045] hover:text-white/75"}`}>
                  <PixelAvatar name={item.seed} className="size-4" /><span className="truncate font-medium">{item.name}</span>{item.id === activeId && <MoreHorizontal className="ml-auto size-3 text-white/35" />}
                </button>
              ))}
            </div>
          </div>
          <div className="border-t border-white/8 p-3"><div className="flex items-center gap-2 font-medium"><PixelAvatar name="Local workspace" className="size-5" />Local workspace<ChevronDown className="ml-auto size-3 text-white/30" /></div></div>
        </aside>

        <section className="flex min-w-0 flex-col">
          <header className="flex h-10 items-center gap-2 border-b border-white/10 px-3">
            <PixelAvatar name={channel.seed} className="size-5" /><span className="font-semibold">{channel.name}</span><span className="text-[9px] text-white/30">{channel.agents.length + 1} members · {channel.topic}</span>
            <button type="button" aria-label="Open chat details" onClick={() => setDetailsOpen(true)} className="ml-auto grid size-6 place-items-center rounded text-white/32 hover:bg-white/5 hover:text-white"><Info className="size-3.5" /></button>
          </header>

          <div className="flex-1 space-y-5 overflow-y-auto p-5">
            {channelMessages.length === 0 ? (
              <div className="grid h-full place-items-center text-center"><div><PixelAvatar name={channel.seed} className="mx-auto mb-3 size-10" /><p className="font-semibold">Start the conversation</p><p className="mt-1 text-[10px] text-white/35">Messages you send here stay in this demo.</p></div></div>
            ) : channelMessages.map((message) => {
              const agent = channel.agents.find((item) => item.name === message.author);
              return (
                <div key={message.id} className="flex gap-3">
                  {message.author === "You" ? <PixelAvatar name="You" className="size-7" /> : agent ? <ProviderGlyph provider={agent.provider} /> : <PixelAvatar name={message.author} className="size-7" />}
                  <div><div className="mb-1 font-semibold">{message.author}<span className="ml-2 font-mono text-[8px] font-normal text-white/22">{message.time}</span></div><p className="max-w-[650px] text-[10px] leading-relaxed text-white/58">{message.text}</p></div>
                </div>
              );
            })}
          </div>

          <form onSubmit={sendMessage} className="border-t border-white/10 p-3">
            <div className="flex min-h-10 items-center gap-2 rounded border border-white/12 bg-white/[0.025] px-3 focus-within:border-white/25">
              <input value={draft} onChange={(event) => setDraft(event.target.value)} placeholder={`Message ${channel.name}`} className="min-w-0 flex-1 bg-transparent text-[10px] text-white outline-none placeholder:text-white/25" />
              <button type="submit" disabled={!draft.trim()} className="grid size-7 place-items-center rounded bg-white text-black transition-opacity disabled:opacity-30"><Send className="size-3.5 -translate-x-px rotate-45" /></button>
            </div>
          </form>
        </section>

        <aside className={`flex min-h-0 flex-col overflow-hidden border-l border-white/10 bg-white/[0.025] transition-opacity max-lg:hidden ${detailsOpen ? "opacity-100" : "opacity-0"}`}>
          <div className="flex h-10 items-center border-b border-white/10 px-3 font-semibold"><span>Chat details</span><button type="button" onClick={() => setDetailsOpen(false)} className="ml-auto grid size-6 place-items-center text-white/35 hover:text-white"><X className="size-3.5" /></button></div>
          <div className="flex-1 overflow-y-auto">
            <section className="flex flex-col items-center gap-2.5 border-b border-white/10 px-3 py-4">
              <PixelAvatar name={channel.seed} className="size-11" />
              <input aria-label="Chat name" value={channel.name} onChange={(event) => patchChannel({ name: event.target.value })} className="h-7 w-full rounded border border-white/14 bg-white/[0.035] px-2 text-center font-semibold outline-none focus:border-white/30" />
              <input aria-label="Chat topic" value={channel.topic} onChange={(event) => patchChannel({ topic: event.target.value })} className="h-7 w-full rounded border border-white/14 bg-white/[0.035] px-2 text-center text-[10px] outline-none focus:border-white/30" />
              <button type="button" onClick={() => patchChannel({ seed: `${channel.seed}-${Date.now()}` })} className="flex items-center gap-1.5 py-1 text-[9px] text-white/35 hover:text-white"><Dices className="size-3" />Shuffle avatar</button>
            </section>

            <section className="border-b border-white/10 p-3">
              <div className="mb-2.5 flex items-center"><div><p className="font-semibold">Agents</p><p className="text-[9px] text-white/32">Choose what wakes each agent</p></div><button type="button" onClick={() => setInviteOpen(true)} className="ml-auto grid size-6 place-items-center text-white/35 hover:text-white"><Plus className="size-3.5" /></button></div>
              <div className="space-y-1.5">
                {channel.agents.length === 0 && <button type="button" onClick={() => setInviteOpen(true)} className="w-full rounded border border-dashed border-white/14 px-2 py-4 text-[10px] text-white/35 hover:border-white/25 hover:text-white">Connect the first agent</button>}
                {channel.agents.map((agent) => (
                  <div key={agent.id} className="rounded border border-white/10 bg-white/[0.025] p-2">
                    <div className="flex items-center gap-2"><ProviderGlyph provider={agent.provider} /><div className="min-w-0 flex-1"><p className="truncate font-medium">{agent.name}</p><p className="flex items-center gap-1 text-[8px] text-white/30"><span className="size-1 rounded-full bg-emerald-400" />online</p></div><button type="button" onClick={() => cycleTrigger(agent.id)} className="flex h-6 items-center gap-1 rounded border border-white/12 px-1.5 text-[8px] text-white/42 hover:text-white">{agent.trigger}<ChevronDown className="size-2.5" /></button></div>
                    <div className="mt-2 flex justify-end border-t border-white/8 pt-1.5"><button type="button" onClick={() => disconnect(agent.id)} className="flex items-center gap-1 text-[8px] text-white/28 hover:text-red-300"><Unplug className="size-2.5" />Disconnect</button></div>
                  </div>
                ))}
              </div>
            </section>

            <section className="p-3"><p className="mb-2 font-semibold">People <span className="text-white/30">1</span></p><div className="flex items-center gap-2"><PixelAvatar name="Local workspace" className="size-5" /><span>Local workspace</span><span className="ml-auto size-1.5 rounded-full bg-emerald-400" /></div></section>
          </div>
          <div className="border-t border-white/10 p-2.5"><button type="button" onClick={() => setInviteOpen(true)} className="flex h-8 w-full items-center justify-center gap-1.5 rounded border border-white/14 text-[10px] hover:bg-white/5"><Plus className="size-3" />Invite agent</button></div>
        </aside>
      </div>

      <div className="flex h-6 items-center border-t border-white/10 px-3 font-mono text-[8px] text-white/32"><span className="mr-1.5 size-1.5 rounded-full bg-emerald-400" />connected · relay<span className="ml-5 mr-1.5 size-1.5 rounded-full bg-emerald-400" />1 gateway online<span className="ml-auto">⌘K agent &nbsp; ⌘J caddy &nbsp; ⌘1–9 channels</span></div>

      {inviteOpen && (
        <div className="absolute inset-0 z-20 grid place-items-center bg-black/70 p-5 backdrop-blur-[2px]" onMouseDown={(event) => event.target === event.currentTarget && setInviteOpen(false)}>
          <form onSubmit={addAgent} className="w-full max-w-sm rounded-lg border border-white/14 bg-[#151619] p-4 shadow-2xl">
            <div className="flex items-start"><div><h3 className="text-sm font-semibold">Connect an agent</h3><p className="mt-1 text-[10px] text-white/38">Add an agent to {channel.name}.</p></div><button type="button" onClick={() => setInviteOpen(false)} className="ml-auto text-white/35 hover:text-white"><X className="size-4" /></button></div>
            <div className="mt-4 grid grid-cols-3 gap-1 rounded border border-white/10 bg-black/20 p-1">
              {(["Claude", "Codex", "Hermes"] as const).map((item) => <button key={item} type="button" onClick={() => { setProvider(item); setAgentName(`${item.toLowerCase()}-main`); }} className={`flex items-center justify-center gap-1.5 rounded px-2 py-2 text-[10px] ${provider === item ? "bg-white/10 text-white" : "text-white/35 hover:text-white/60"}`}>{provider === item && <Check className="size-3" />}{item}</button>)}
            </div>
            <label className="mt-4 block text-[9px] font-medium text-white/50">Agent name<input value={agentName} onChange={(event) => setAgentName(event.target.value)} className="mt-1.5 h-9 w-full rounded border border-white/12 bg-white/[0.035] px-3 text-[11px] text-white outline-none focus:border-white/30" /></label>
            <button type="submit" className="mt-4 h-9 w-full rounded bg-white text-[10px] font-semibold text-black hover:bg-[#dce3ff]">Connect agent</button>
          </form>
        </div>
      )}
    </div>
  );
}
