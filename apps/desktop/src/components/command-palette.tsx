import { useEffect, useState } from "react";
import { MessageSquareIcon, PlusIcon, AtSignIcon, SparklesIcon } from "@/components/ui/icons";
import { mockBrain } from "@/lib/brain";
import { runPlan } from "@/lib/commands";
import {
  Command,
  CommandDialog,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
  CommandSeparator,
  CommandShortcut,
} from "@/components/ui/command";
import { MemberAvatar } from "@/components/member-avatar";
import { useChat } from "@/lib/use-chat";

export function CommandPalette() {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const { channels, members, selectChannel, setInviteOpen, setComposerInsert } =
    useChat();
  const plan = query.trim().length > 2 ? mockBrain.plan(query) : null;

  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "k" && (e.metaKey || e.ctrlKey)) {
        e.preventDefault();
        setOpen((o) => !o);
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);

  const run = (action: () => void) => {
    setOpen(false);
    action();
  };

  return (
    <CommandDialog open={open} onOpenChange={setOpen}>
      <Command>
      <CommandInput
        value={query}
        onValueChange={setQuery}
        placeholder="Jump somewhere, or just say what you want done…"
      />
      <CommandList>
        <CommandEmpty>Nothing matches.</CommandEmpty>
        {plan && (
          <CommandGroup heading="Agent">
            <CommandItem
              value={query}
              onSelect={() => {
                setOpen(false);
                setQuery("");
                void runPlan(plan).catch(() => {});
              }}
            >
              <SparklesIcon className="size-4 text-violet-400" />
              <span className="truncate">
                {plan.map((c) => c.label).join(" → ")}
              </span>
            </CommandItem>
          </CommandGroup>
        )}
        <CommandGroup heading="Chats">
          {channels.map((channel, i) => (
            <CommandItem
              key={channel.id}
              value={`channel ${channel.name}`}
              onSelect={() => run(() => void selectChannel(channel.id))}
            >
              <MessageSquareIcon className="size-4 text-muted-foreground" />
              <span>{channel.name}</span>
              {channel.topic && (
                <span className="truncate text-xs text-muted-foreground">
                  {channel.topic}
                </span>
              )}
              <CommandShortcut>⌘{i + 1}</CommandShortcut>
            </CommandItem>
          ))}
        </CommandGroup>
        <CommandSeparator />
        <CommandGroup heading="Mention">
          {members.map((member) => (
            <CommandItem
              key={member.id}
              value={`mention ${member.name} ${member.type}`}
              onSelect={() =>
                run(() => setComposerInsert(`@${member.name} `))
              }
            >
              <AtSignIcon className="size-4 text-muted-foreground" />
              <MemberAvatar member={member} className="size-5" />
              <span>{member.name}</span>
              <span className="ml-auto text-[10px] uppercase text-muted-foreground">
                {member.type}
              </span>
            </CommandItem>
          ))}
        </CommandGroup>
        <CommandSeparator />
        <CommandGroup heading="Workspace">
          <CommandItem
            value="invite agent human"
            onSelect={() => run(() => setInviteOpen(true))}
          >
            <PlusIcon className="size-4 text-muted-foreground" />
            <span>Invite people or agents</span>
          </CommandItem>
        </CommandGroup>
      </CommandList>
      </Command>
    </CommandDialog>
  );
}
