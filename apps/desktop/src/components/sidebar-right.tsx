import { useEffect, useMemo, useState } from "react";
import type { Trigger } from "@agentchat/shared";
import {
  CheckIcon,
  ChevronDownIcon,
  DicesIcon,
  LoaderCircleIcon,
  PlusIcon,
  RotateCcwIcon,
  UnplugIcon,
  XIcon,
} from "@/components/ui/icons";
import { AvatarFrame, PresenceDot } from "@/components/member-avatar";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Input } from "@/components/ui/input";
import { Separator } from "@/components/ui/separator";
import {
  Sidebar,
  SidebarContent,
  SidebarFooter,
  SidebarHeader,
} from "@/components/ui/sidebar";
import { memberById, store, useChat } from "@/lib/use-chat";
import { cn } from "@/lib/utils";

const triggerOptions: { value: Trigger; label: string; detail: string }[] = [
  { value: "all", label: "Every message", detail: "Always wakes" },
  { value: "mention", label: "Mentions", detail: "Only when @mentioned" },
  { value: "never", label: "Muted", detail: "Reads without waking" },
];

function TriggerPicker({
  value,
  disabled,
  onChange,
}: {
  value: Trigger;
  disabled?: boolean;
  onChange: (trigger: Trigger) => void;
}) {
  const selected = triggerOptions.find((option) => option.value === value)!;
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <button
          type="button"
          disabled={disabled}
          className="flex h-7 items-center gap-1 rounded-md border border-border/70 bg-background px-2 text-[11px] text-muted-foreground transition-colors hover:bg-muted hover:text-foreground disabled:opacity-50"
        >
          {selected.label}
          {disabled ? (
            <LoaderCircleIcon className="size-3 animate-spin" />
          ) : (
            <ChevronDownIcon className="size-3" />
          )}
        </button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="w-52">
        {triggerOptions.map((option) => (
          <DropdownMenuItem
            key={option.value}
            onSelect={() => onChange(option.value)}
            className="items-start gap-2"
          >
            <CheckIcon
              className={cn(
                "mt-0.5 size-3.5",
                option.value !== value && "invisible",
              )}
            />
            <span className="flex flex-col">
              <span>{option.label}</span>
              <span className="text-[10px] text-muted-foreground">
                {option.detail}
              </span>
            </span>
          </DropdownMenuItem>
        ))}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

export function SidebarRight({
  className,
  ...props
}: React.ComponentProps<typeof Sidebar>) {
  const {
    channels,
    activeChannelId,
    members,
    setInviteOpen,
    setRightSidebarOpen,
    updateChannel,
    kickMember,
    resetAgentSession,
  } = useChat();
  const channel = channels.find((candidate) => candidate.id === activeChannelId);
  const [name, setName] = useState("");
  const [topic, setTopic] = useState("");
  const [triggers, setTriggers] = useState<Record<string, Trigger>>({});
  const [action, setAction] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const chatMembers = useMemo(
    () =>
      (channel?.memberIds ?? [])
        .map((id) => memberById(members, id))
        .filter((member): member is NonNullable<typeof member> => Boolean(member)),
    [channel?.memberIds, members],
  );
  const agents = chatMembers.filter((member) => member.type === "agent");
  const humans = chatMembers.filter((member) => member.type === "human");
  const agentKey = agents.map((agent) => agent.id).join(":");

  useEffect(() => {
    setName(channel?.name ?? "");
    setTopic(channel?.topic ?? "");
    setNotice(null);
  }, [channel?.id, channel?.name, channel?.topic]);

  useEffect(() => {
    if (!channel || agents.length === 0) {
      setTriggers({});
      return;
    }
    let cancelled = false;
    void Promise.all(
      agents.map(async (agent) => {
        const memberships = await store.listMemberships(agent.id);
        return [
          agent.id,
          memberships.find((membership) => membership.chat === channel.id)
            ?.trigger ?? "mention",
        ] as const;
      }),
    )
      .then((entries) => {
        if (!cancelled) setTriggers(Object.fromEntries(entries));
      })
      .catch((error) => {
        if (!cancelled) {
          setNotice(error instanceof Error ? error.message : String(error));
        }
      });
    return () => {
      cancelled = true;
    };
    // agentKey intentionally captures membership changes without depending on
    // the derived agents array identity.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [channel?.id, agentKey]);

  if (!channel) {
    return (
      <Sidebar
        side="right"
        collapsible="none"
        className={cn("sticky top-0 hidden h-svh border-l lg:flex", className)}
        {...props}
      >
        <SidebarHeader className="h-11 justify-center border-b px-4">
          <span className="text-xs font-medium text-muted-foreground">Chat details</span>
        </SidebarHeader>
        <SidebarContent className="grid place-items-center px-5 text-center text-xs text-muted-foreground">
          Select a chat to see its details.
        </SidebarContent>
      </Sidebar>
    );
  }

  const isGroup = channel.kind === "group";
  const dmAgent = !isGroup ? agents[0] : undefined;
  const avatarSeed = channel.avatarSeed ?? dmAgent?.name ?? channel.name;

  const commitName = () => {
    const next = name.trim();
    if (next && next !== channel.name) {
      void updateChannel(channel.id, { name: next }).catch((error) =>
        setNotice(error instanceof Error ? error.message : String(error)),
      );
    }
  };

  const commitTopic = () => {
    const next = topic.trim();
    if (next !== (channel.topic ?? "")) {
      void updateChannel(channel.id, { topic: next }).catch((error) =>
        setNotice(error instanceof Error ? error.message : String(error)),
      );
    }
  };

  const changeTrigger = async (memberId: string, trigger: Trigger) => {
    const previous = triggers[memberId] ?? "mention";
    setTriggers((current) => ({ ...current, [memberId]: trigger }));
    setAction(`trigger:${memberId}`);
    setNotice(null);
    try {
      await store.setMemberTrigger(channel.id, memberId, trigger);
    } catch (error) {
      setTriggers((current) => ({ ...current, [memberId]: previous }));
      setNotice(error instanceof Error ? error.message : String(error));
    } finally {
      setAction(null);
    }
  };

  const resetSession = async (memberId: string, memberName: string) => {
    if (!window.confirm(`Reset ${memberName}'s private session in ${channel.name}? The shared chat stays intact.`)) return;
    setAction(`reset:${memberId}`);
    setNotice(null);
    try {
      await resetAgentSession(channel.id, memberId);
      setNotice(`${memberName} will start fresh on the next message.`);
    } catch (error) {
      setNotice(error instanceof Error ? error.message : String(error));
    } finally {
      setAction(null);
    }
  };

  const disconnect = async (memberId: string, memberName: string) => {
    if (!window.confirm(`Disconnect ${memberName} from ${channel.name}?`)) return;
    setAction(`disconnect:${memberId}`);
    setNotice(null);
    try {
      await kickMember(channel.id, memberId);
      setNotice(`${memberName} disconnected.`);
    } catch (error) {
      setNotice(error instanceof Error ? error.message : String(error));
    } finally {
      setAction(null);
    }
  };

  return (
    <Sidebar
      side="right"
      collapsible="none"
      className={cn("sticky top-0 hidden h-svh border-l lg:flex", className)}
      {...props}
    >
      <SidebarHeader className="flex h-11 flex-row items-center border-b px-4">
        <span className="text-xs font-semibold">Chat details</span>
        <button
          type="button"
          aria-label="Close chat details"
          onClick={() => setRightSidebarOpen(false)}
          className="ml-auto grid size-6 place-items-center rounded-md text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
        >
          <XIcon className="size-3.5" />
        </button>
      </SidebarHeader>

      <SidebarContent className="gap-0">
        <section className="flex flex-col items-center gap-3 px-4 py-5 text-center">
          <AvatarFrame name={avatarSeed} className="size-14" />
          {isGroup ? (
            <div className="w-full space-y-2">
              <Input
                value={name}
                onChange={(event) => setName(event.target.value)}
                onBlur={commitName}
                onKeyDown={(event) => event.key === "Enter" && commitName()}
                aria-label="Chat name"
                className="h-8 text-center text-sm font-medium"
              />
              <Input
                value={topic}
                onChange={(event) => setTopic(event.target.value)}
                onBlur={commitTopic}
                onKeyDown={(event) => event.key === "Enter" && commitTopic()}
                placeholder="Add a topic"
                aria-label="Chat topic"
                className="h-8 text-center text-xs"
              />
              <Button
                size="sm"
                variant="ghost"
                className="h-7 text-xs text-muted-foreground"
                onClick={() =>
                  void updateChannel(channel.id, {
                    avatarSeed: `seed-${Math.random().toString(36).slice(2, 8)}`,
                  }).catch((error) =>
                    setNotice(error instanceof Error ? error.message : String(error)),
                  )
                }
              >
                <DicesIcon className="size-3.5" /> Shuffle avatar
              </Button>
            </div>
          ) : (
            <div>
              <p className="text-sm font-semibold">{channel.name}</p>
              <p className="mt-1 text-xs text-muted-foreground">
                {dmAgent?.bio ?? "Direct chat"}
              </p>
            </div>
          )}
        </section>

        <Separator />

        <section className="px-3 py-4">
          <div className="mb-3 flex items-center justify-between px-1">
            <div>
              <p className="text-xs font-medium">Agents</p>
              <p className="text-[10px] text-muted-foreground">
                Choose what wakes each agent
              </p>
            </div>
            <button
              type="button"
              onClick={() => setInviteOpen(true)}
              className="grid size-7 place-items-center rounded-md text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
              title="Invite an agent"
            >
              <PlusIcon className="size-4" />
            </button>
          </div>

          <div className="space-y-1.5">
            {agents.length === 0 && (
              <button
                type="button"
                onClick={() => setInviteOpen(true)}
                className="w-full rounded-lg border border-dashed px-3 py-4 text-xs text-muted-foreground transition-colors hover:border-foreground/25 hover:text-foreground"
              >
                Connect the first agent
              </button>
            )}
            {agents.map((agent) => (
              <div key={agent.id} className="group rounded-lg border border-border/60 bg-muted/20 p-2.5">
                <div className="flex items-center gap-2">
                  <AvatarFrame name={agent.name} className="size-7" />
                  <div className="min-w-0 flex-1">
                    <p className="truncate text-xs font-medium">{agent.name}</p>
                    <p className="flex items-center gap-1.5 text-[10px] capitalize text-muted-foreground">
                      <PresenceDot member={agent} /> {agent.presence}
                    </p>
                  </div>
                  <TriggerPicker
                    value={triggers[agent.id] ?? "mention"}
                    disabled={action === `trigger:${agent.id}`}
                    onChange={(trigger) => void changeTrigger(agent.id, trigger)}
                  />
                </div>
                <div className="mt-2 flex justify-end gap-1 border-t border-border/50 pt-2">
                  <button
                    type="button"
                    disabled={action !== null}
                    onClick={() => void resetSession(agent.id, agent.name)}
                    className="flex h-6 items-center gap-1 rounded px-1.5 text-[10px] text-muted-foreground transition-colors hover:bg-muted hover:text-foreground disabled:opacity-40"
                  >
                    {action === `reset:${agent.id}` ? <LoaderCircleIcon className="size-3 animate-spin" /> : <RotateCcwIcon className="size-3" />}
                    Reset
                  </button>
                  <button
                    type="button"
                    disabled={action !== null}
                    onClick={() => void disconnect(agent.id, agent.name)}
                    className="flex h-6 items-center gap-1 rounded px-1.5 text-[10px] text-muted-foreground transition-colors hover:bg-muted hover:text-foreground disabled:opacity-40"
                  >
                    {action === `disconnect:${agent.id}` ? <LoaderCircleIcon className="size-3 animate-spin" /> : <UnplugIcon className="size-3" />}
                    Disconnect
                  </button>
                </div>
              </div>
            ))}
          </div>
        </section>

        {humans.length > 0 && (
          <>
            <Separator />
            <section className="px-3 py-4">
              <p className="mb-2 px-1 text-xs font-medium">
                People <span className="text-muted-foreground">{humans.length}</span>
              </p>
              <div className="space-y-1">
                {humans.map((human) => (
                  <div key={human.id} className="flex items-center gap-2 rounded-md px-1 py-1.5">
                    <AvatarFrame name={human.name} className="size-6" />
                    <span className="min-w-0 flex-1 truncate text-xs">{human.name}</span>
                    <PresenceDot member={human} />
                  </div>
                ))}
              </div>
            </section>
          </>
        )}

        {notice && (
          <p className="mx-3 mb-3 rounded-md border border-border/70 bg-muted/30 px-2.5 py-2 text-[11px] text-muted-foreground">
            {notice}
          </p>
        )}
      </SidebarContent>

      <SidebarFooter className="border-t p-3">
        <Button size="sm" variant="outline" className="w-full" onClick={() => setInviteOpen(true)}>
          <PlusIcon className="size-4" /> Invite agent
        </Button>
      </SidebarFooter>
    </Sidebar>
  );
}
