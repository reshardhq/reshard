import * as React from "react";
import {
  BellOffIcon,
  CopyIcon,
  HashIcon,
  PlusIcon,
  ZapIcon,
  BoxIcon,
} from "@/components/ui/icons";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuShortcut,
  ContextMenuTrigger,
} from "@/components/ui/context-menu";
import {
  Sidebar,
  SidebarContent,
  SidebarFooter,
  SidebarGroup,
  SidebarGroupContent,
  SidebarGroupLabel,
  SidebarHeader,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
} from "@/components/ui/sidebar";
import { TeamSwitcher } from "@/components/team-switcher";
import { useChat } from "@/lib/use-chat";
import { cn } from "@/lib/utils";

const WORKSPACES = [
  { name: "t31k's workspace", logo: <ZapIcon className="size-3" />, plan: "" },
  { name: "pixelated", logo: <BoxIcon className="size-3" />, plan: "" },
];

export function AppSidebar(props: React.ComponentProps<typeof Sidebar>) {
  const { channels, activeChannelId, selectChannel, setInviteOpen, agentTouch } =
    useChat();

  return (
    <Sidebar {...props}>
      <SidebarHeader className="pt-2">
        <TeamSwitcher teams={WORKSPACES} />
      </SidebarHeader>

      <SidebarContent>
        <SidebarGroup>
          <SidebarGroupLabel>Channels</SidebarGroupLabel>
          <SidebarGroupContent>
            <SidebarMenu>
              {channels.map((channel, i) => (
                <SidebarMenuItem key={channel.id}>
                  <ContextMenu>
                    <ContextMenuTrigger asChild>
                      <SidebarMenuButton
                        isActive={channel.id === activeChannelId}
                        onClick={() => selectChannel(channel.id)}
                        className={cn(
                          agentTouch === `channel:${channel.id}` && "agent-touch",
                        )}
                      >
                        <HashIcon className="size-3.5 text-muted-foreground" />
                        <span>{channel.name}</span>
                      </SidebarMenuButton>
                    </ContextMenuTrigger>
                    <ContextMenuContent className="w-48">
                      <ContextMenuItem
                        onSelect={() => void selectChannel(channel.id)}
                      >
                        <HashIcon className="size-4" /> Open channel
                        <ContextMenuShortcut>⌘{i + 1}</ContextMenuShortcut>
                      </ContextMenuItem>
                      <ContextMenuItem
                        onSelect={() =>
                          void navigator.clipboard.writeText(`#${channel.name}`)
                        }
                      >
                        <CopyIcon className="size-4" /> Copy name
                      </ContextMenuItem>
                      <ContextMenuSeparator />
                      <ContextMenuItem disabled>
                        <BellOffIcon className="size-4" /> Mute channel
                      </ContextMenuItem>
                    </ContextMenuContent>
                  </ContextMenu>
                </SidebarMenuItem>
              ))}
            </SidebarMenu>
          </SidebarGroupContent>
        </SidebarGroup>
      </SidebarContent>

      <SidebarFooter>
        <SidebarMenu>
          <SidebarMenuItem>
            <SidebarMenuButton onClick={() => setInviteOpen(true)}>
              <PlusIcon className="size-4" />
              <span>New channel</span>
            </SidebarMenuButton>
          </SidebarMenuItem>
        </SidebarMenu>
      </SidebarFooter>
    </Sidebar>
  );
}
