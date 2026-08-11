import { MessageList } from "@/components/chat/message-list";
import { Composer } from "@/components/chat/composer";
import { LoadingState } from "@/components/ai";
import { InfoIcon } from "@/components/ui/icons";
import { Button } from "@/components/ui/button";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { AvatarFrame, PresenceDot } from "@/components/member-avatar";
import { ActivitySheet } from "@/components/activity-sheet";
import { LiveTurn } from "@/components/chat/live-turn";
import { useChat, memberById } from "@/lib/use-chat";

function WorkingIndicator() {
  const { working, activeChannelId, members } = useChat();
  const entry = working.find((w) => w.channelId === activeChannelId);
  if (!entry) return null;
  const member = memberById(members, entry.memberId);
  return (
    <div className="px-6 pb-1.5">
      <LoadingState label={`${member?.name ?? "agent"} is working`} />
    </div>
  );
}

export function ChatView() {
  const { channels, activeChannelId, members } = useChat();
  const channel = channels.find((c) => c.id === activeChannelId);
  const dmMember =
    channel?.kind === "dm"
      ? members.find(
          (m) => channel.memberIds.includes(m.id) && m.type === "agent",
        )
      : undefined;

  if (!channel) {
    return (
      <div className="flex h-full min-h-0 flex-col items-center justify-center gap-3 px-6 text-center">
        <div className="font-mono text-xs uppercase tracking-[0.18em] text-muted-foreground">Your workspace is ready</div>
        <h1 className="text-xl font-semibold">Start your first chat</h1>
        <p className="max-w-sm text-balance text-sm text-muted-foreground">
          Create a chat now. You’ll connect an agent to it in the next step.
        </p>
        <Button onClick={() => useChat.getState().setNewChatOpen(true)}>Create chat</Button>
      </div>
    );
  }

  return (
    <div className="relative flex h-full min-h-0 flex-col overflow-hidden">
      <header className="flex h-10 shrink-0 items-center gap-2.5 border-b px-4">
        {channel && (
          <AvatarFrame
            name={dmMember?.name ?? channel.avatarSeed ?? channel.name}
            className="size-6"
          />
        )}
        <span className="text-sm font-semibold capitalize">
          {channel?.name ?? "…"}
        </span>
        {dmMember ? (
          <span className="flex items-center gap-1.5 text-xs text-muted-foreground">
            <PresenceDot member={dmMember} />
            {dmMember.presence}
          </span>
        ) : (
          channel && (
            <span className="ml-1 truncate text-xs text-muted-foreground">
              {channel.memberIds.length} members
              {channel.topic ? ` · ${channel.topic}` : ""}
            </span>
          )
        )}
        <span className="ml-auto flex items-center">
          <TooltipProvider>
          <Tooltip>
            <TooltipTrigger asChild>
              <button
                aria-label="Chat info"
                onClick={() => useChat.getState().setRightSidebarOpen(true)}
                className="flex size-6 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-muted/50 hover:text-foreground"
              >
                <InfoIcon className="size-4" />
              </button>
            </TooltipTrigger>
            <TooltipContent side="bottom">Chat details</TooltipContent>
          </Tooltip>
          </TooltipProvider>
        </span>
      </header>
      <MessageList />
      <WorkingIndicator />
      <LiveTurn />
      <Composer />
      <ActivitySheet />
    </div>
  );
}
