import { useEffect, useState } from "react";
import type { IconType } from "react-icons";
import { VscOpenai } from "react-icons/vsc";
import { SiClaudecode } from "react-icons/si";
import { GiWingfoot } from "react-icons/gi";
import { BotIcon, CheckIcon, CopyIcon } from "@/components/ui/icons";
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
import { useChat } from "@/lib/use-chat";
import { createAgent, createPairing } from "@/lib/auth";

type AgentProvider = "claude" | "codex" | "hermes";

const providers: Array<{ id: AgentProvider; label: string; icon: IconType }> = [
  { id: "claude", label: "Claude", icon: SiClaudecode },
  { id: "codex", label: "Codex", icon: VscOpenai },
  { id: "hermes", label: "Hermes", icon: GiWingfoot },
];

const INSTALL_CMD = "curl -fsSL https://reshard.dev/install.sh | sh";

export function InviteModal() {
  const { inviteOpen, setInviteOpen, channels, activeChannelId, machines, refreshMachines } =
    useChat();
  const channel = channels.find((c) => c.id === activeChannelId);

  const [provider, setProvider] = useState<AgentProvider>("claude");
  const [name, setName] = useState("");
  const [machineId, setMachineId] = useState("");
  const [cwd, setCwd] = useState("");
  const [pairingCode, setPairingCode] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!inviteOpen) return;
    setProvider("claude");
    setName("");
    setCwd("");
    setError(null);
    setBusy(false);
    void refreshMachines();
    if (machines.length === 0) {
      void createPairing().then(setPairingCode).catch(() => {});
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [inviteOpen]);

  // Default the machine picker to the first machine once the list loads.
  useEffect(() => {
    if (inviteOpen && !machineId && machines[0]) setMachineId(machines[0].id);
  }, [machines, inviteOpen, machineId]);

  async function add() {
    if (!activeChannelId || !machineId) return;
    setBusy(true);
    setError(null);
    try {
      await createAgent({
        name: name.trim() || `${provider}-bot`,
        runtime: provider,
        machineId,
        cwd: cwd.trim() || undefined,
        chatId: activeChannelId,
      });
      setInviteOpen(false);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }

  const noMachine = machines.length === 0;

  return (
    <Dialog open={inviteOpen} onOpenChange={setInviteOpen}>
      <DialogContent className="border-border/90 bg-card shadow-2xl sm:max-w-md">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <BotIcon className="size-4" /> Add an agent
          </DialogTitle>
          <DialogDescription>
            {noMachine
              ? "Pair a machine first to run agents."
              : `Add an agent to ${channel ? channel.name : "this chat"}.`}
          </DialogDescription>
        </DialogHeader>

        {noMachine ? (
          <div className="space-y-3 pt-1 text-sm">
            <p className="text-muted-foreground">
              No paired machines yet. On the machine where your agents run:
            </p>
            <Cmd label="1 · Install the CLI" command={INSTALL_CMD} />
            <Cmd
              label="2 · Pair this machine"
              command={pairingCode ? `reshard setup ${pairingCode}` : "Preparing code…"}
            />
          </div>
        ) : (
          <div className="space-y-4 pt-1">
            <div className="space-y-2">
              <Label>Runtime</Label>
              <div
                className="grid grid-cols-3 gap-1 rounded-lg border border-border/80 bg-background/40 p-1"
                role="radiogroup"
                aria-label="Agent runtime"
              >
                {providers.map((option) => {
                  const selected = provider === option.id;
                  const Icon = option.icon;
                  return (
                    <button
                      key={option.id}
                      type="button"
                      role="radio"
                      aria-checked={selected}
                      onClick={() => setProvider(option.id)}
                      className={`flex items-center justify-center gap-2 rounded-md px-3 py-2 text-xs font-medium transition-all focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring ${
                        selected
                          ? "bg-muted text-foreground shadow-sm"
                          : "text-muted-foreground hover:bg-muted/50 hover:text-foreground"
                      }`}
                    >
                      <Icon className="size-4" aria-hidden="true" />
                      {option.label}
                    </button>
                  );
                })}
              </div>
            </div>

            <div className="space-y-2">
              <Label htmlFor="machine">Machine</Label>
              {machines.length === 1 ? (
                <div className="rounded-md border bg-background/40 px-3 py-2 text-sm">
                  {machines[0].name}
                </div>
              ) : (
                <select
                  id="machine"
                  value={machineId}
                  onChange={(e) => setMachineId(e.target.value)}
                  className="w-full rounded-md border bg-background/40 px-3 py-2 text-sm outline-none focus-visible:ring-1 focus-visible:ring-ring"
                >
                  {machines.map((m) => (
                    <option key={m.id} value={m.id}>
                      {m.name}
                      {m.online ? "" : " (offline)"}
                    </option>
                  ))}
                </select>
              )}
            </div>

            <div className="space-y-2">
              <Label htmlFor="cwd">Project directory</Label>
              <Input
                id="cwd"
                value={cwd}
                onChange={(e) => setCwd(e.target.value)}
                placeholder="/home/you/projects/foo"
                className="font-mono text-xs"
              />
              <p className="text-xs text-muted-foreground">
                The agent runs here — reads its CLAUDE.md and code. Blank = home dir.
              </p>
            </div>

            <div className="space-y-2">
              <Label htmlFor="agent-name">Name</Label>
              <Input
                id="agent-name"
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder={`e.g. ${provider}-bot`}
              />
            </div>

            {error && <p className="text-xs text-destructive">{error}</p>}

            <Button className="w-full" disabled={busy || !machineId} onClick={() => void add()}>
              {busy ? "Adding…" : "Add agent"}
            </Button>
          </div>
        )}
      </DialogContent>
    </Dialog>
  );
}

function Cmd({ label, command }: { label: string; command: string }) {
  const [copied, setCopied] = useState(false);
  return (
    <div className="space-y-1">
      <span className="text-xs font-medium text-muted-foreground">{label}</span>
      <div className="relative rounded-md bg-muted/40 px-2.5 py-2 font-mono text-xs text-muted-foreground">
        <pre className="whitespace-pre-wrap">{command}</pre>
        <Button
          size="icon"
          variant="ghost"
          className="absolute right-1 top-1 size-7"
          onClick={() =>
            void navigator.clipboard.writeText(command).then(() => {
              setCopied(true);
              setTimeout(() => setCopied(false), 1500);
            })
          }
        >
          {copied ? (
            <CheckIcon className="size-3.5 text-emerald-400" />
          ) : (
            <CopyIcon className="size-3.5" />
          )}
        </Button>
      </div>
    </div>
  );
}
