import { useEffect, useState } from "react";
import type { Invite } from "@agentchat/shared";
import { BotIcon, CheckIcon, CopyIcon } from "@/components/ui/icons";
import type { IconType } from "react-icons";
import { VscOpenai } from "react-icons/vsc";
import { SiClaudecode } from "react-icons/si";
import { GiWingfoot } from "react-icons/gi";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { createInvite, useChat } from "@/lib/use-chat";
import { createPairing } from "@/lib/auth";

type AgentProvider = "claude" | "codex" | "hermes";

const providers: Array<{ id: AgentProvider; label: string; icon: IconType }> = [
  { id: "claude", label: "Claude", icon: SiClaudecode },
  { id: "codex", label: "Codex", icon: VscOpenai },
  { id: "hermes", label: "Hermes", icon: GiWingfoot },
];

export function InviteModal() {
  const { inviteOpen, setInviteOpen, channels, activeChannelId } = useChat();
  const [invite, setInvite] = useState<Invite | null>(null);
  const [copied, setCopied] = useState(false);
  const [agentName, setAgentName] = useState("");
  const [provider, setProvider] = useState<AgentProvider>("claude");
  const [pairingCode, setPairingCode] = useState("");
  const [error, setError] = useState<string | null>(null);
  const channel = channels.find((c) => c.id === activeChannelId);

  useEffect(() => {
    if (inviteOpen) {
      setInvite(null);
      setError(null);
      setCopied(false);
      setAgentName("");
      setProvider("claude");
      void generate();
      void createPairing().then(setPairingCode).catch((error) => setError(error instanceof Error ? error.message : String(error)));
    }
  }, [inviteOpen, activeChannelId]);

  async function generate() {
    if (!activeChannelId) return;
    setError(null);
    try {
      setInvite(await createInvite(activeChannelId));
      setCopied(false);
    } catch (error) {
      setError(error instanceof Error ? error.message : String(error));
    }
  }

  const snippet = invite
    ? `rebeam connect ${invite.code} --provider ${provider} --name ${agentName.trim() || "<agent-name>"}`
    : "";
  return (
    <Dialog open={inviteOpen} onOpenChange={setInviteOpen}>
      <DialogContent className="border-border/90 bg-card shadow-2xl sm:max-w-xl">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2"><BotIcon className="size-4" /> Connect an agent</DialogTitle>
          <DialogDescription>
            Connect the agent for {channel ? channel.name : "this chat"}.
          </DialogDescription>
        </DialogHeader>
        <div className="space-y-4 pt-2">
          <Instruction number="1" title="Install Rebeam CLI" command="curl -fsSL https://raw.githubusercontent.com/T31K/rebeam/main/install.sh | sh" />
          <Instruction number="2" title="Pair this machine" command={pairingCode ? `rebeam pair ${pairingCode}` : "Preparing pairing command…"} />
          <div className="rounded-lg border p-3">
            <div className="mb-2 flex items-center gap-2 text-sm font-medium"><span className="flex size-5 items-center justify-center rounded-full bg-muted font-mono text-[10px]">3</span> Ask your agent to join</div>
            {invite ? (
              <>
                <div className="mb-3 space-y-2">
                  <Label>Agent</Label>
                  <div
                    className="grid grid-cols-3 gap-1 rounded-lg border border-border/80 bg-background/40 p-1"
                    role="radiogroup"
                    aria-label="Agent provider"
                  >
                    {providers.map((option) => {
                      const selected = provider === option.id;
                      const ProviderIcon = option.icon;
                      return (
                        <button
                          key={option.id}
                          type="button"
                          role="radio"
                          aria-checked={selected}
                          onClick={() => {
                            setProvider(option.id);
                            setCopied(false);
                          }}
                          className={`relative flex items-center justify-center gap-2 rounded-md px-3 py-2 text-xs font-medium transition-all focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring ${
                            selected
                              ? "bg-muted text-foreground shadow-sm"
                              : "text-muted-foreground hover:bg-muted/50 hover:text-foreground"
                          }`}
                        >
                          <span
                            className={`flex size-6 items-center justify-center rounded-md border bg-background/70 transition-colors ${
                              selected ? "border-border" : "border-transparent"
                            }`}
                          >
                            <ProviderIcon className="size-4" aria-hidden="true" />
                          </span>
                          {option.label}
                        </button>
                      );
                    })}
                  </div>
                </div>
                <div className="mb-3 space-y-2">
                  <Label htmlFor="agent-name">Agent name</Label>
                  <Input
                    id="agent-name"
                    autoFocus
                    value={agentName}
                    onChange={(event) => {
                      setAgentName(event.target.value);
                      setCopied(false);
                    }}
                    placeholder={`e.g. ${provider}-main`}
                  />
                </div>
                <p className="mb-2 text-xs text-muted-foreground">Paste this into the terminal where the agent runs.</p>
                <CopyableCommand command={snippet} copied={copied} onCopy={async () => { await navigator.clipboard.writeText(snippet); setCopied(true); setTimeout(() => setCopied(false), 1500); }} />
              </>
            ) : (
              <p className="py-5 text-center text-sm text-muted-foreground">Preparing your agent command…</p>
            )}
          </div>
          {error && <p className="text-xs text-destructive">{error}</p>}
        </div>
      </DialogContent>
    </Dialog>
  );
}

function Instruction({ number, title, command }: { number: string; title: string; command: string }) {
  const [copied, setCopied] = useState(false);
  return <div className="rounded-lg border p-3"><div className="mb-2 flex items-center gap-2 text-sm font-medium"><span className="flex size-5 items-center justify-center rounded-full bg-muted font-mono text-[10px]">{number}</span>{title}</div><div className="relative rounded-md bg-muted/40 px-2.5 py-2 font-mono text-xs text-muted-foreground"><pre className="whitespace-pre-wrap">{command}</pre><Button size="icon" variant="ghost" className="absolute right-1 top-1 size-7" onClick={() => void navigator.clipboard.writeText(command).then(() => { setCopied(true); setTimeout(() => setCopied(false), 1500); })}>{copied ? <CheckIcon className="size-3.5 text-emerald-400" /> : <CopyIcon className="size-3.5" />}</Button></div></div>;
}

function CopyableCommand({ command, copied, onCopy }: { command: string; copied: boolean; onCopy: () => Promise<void> }) {
  return <div className="relative rounded-md bg-muted/40 p-2.5 font-mono text-xs leading-relaxed"><pre className="whitespace-pre-wrap">{command}</pre><Button size="icon" variant="ghost" className="absolute right-1 top-1 size-7" onClick={() => void onCopy()}>{copied ? <CheckIcon className="size-3.5 text-emerald-400" /> : <CopyIcon className="size-3.5" />}</Button></div>;
}
