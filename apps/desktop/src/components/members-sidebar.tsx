import { AtSignIcon, CopyIcon, PlusIcon, UnplugIcon } from "@/components/ui/icons";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuTrigger,
} from "@/components/ui/context-menu";
import {
  Sidebar,
  SidebarContent,
  SidebarFooter,
  SidebarGroup,
  SidebarGroupAction,
  SidebarGroupContent,
  SidebarGroupLabel,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
} from "@/components/ui/sidebar";
import { MemberAvatar, PresenceDot } from "@/components/member-avatar";
import { useChat } from "@/lib/use-chat";
import { cn } from "@/lib/utils";

export function MembersSidebar() {
  const { members, setInviteOpen, setComposerInsert, agentTouch } = useChat();
  const humans = members.filter((m) => m.type === "human");
  const agents = members.filter((m) => m.type === "agent");

  return (
    <Sidebar
      side="right"
      collapsible="none"
      className="hidden h-svh w-56 pb-8 pt-11 lg:flex"
    >
      <SidebarContent>
        <SidebarGroup>
          <SidebarGroupLabel>Agents</SidebarGroupLabel>
          <SidebarGroupAction
            title="Invite an agent"
            onClick={() => setInviteOpen(true)}
          >
            <PlusIcon />
          </SidebarGroupAction>
          <SidebarGroupContent>
            <SidebarMenu>
              {agents.map((agent) => (
                <SidebarMenuItem key={agent.id}>
                  <ContextMenu>
                    <ContextMenuTrigger asChild>
                      <SidebarMenuButton
                        className={cn(
                          "h-9",
                          agentTouch === `member:${agent.id}` && "agent-touch",
                        )}
                      >
                        <MemberAvatar member={agent} className="size-6" />
                        <span
                          className={cn(
                            "flex-1 truncate",
                            agent.presence === "offline" &&
                              "text-muted-foreground",
                          )}
                        >
                          {agent.name}
                        </span>
                        <PresenceDot member={agent} />
                      </SidebarMenuButton>
                    </ContextMenuTrigger>
                    <ContextMenuContent className="w-52">
                      <ContextMenuItem
                        onSelect={() => setComposerInsert(`@${agent.name} `)}
                      >
                        <AtSignIcon className="size-4" /> Mention
                      </ContextMenuItem>
                      <ContextMenuItem
                        onSelect={() =>
                          void navigator.clipboard.writeText(agent.name)
                        }
                      >
                        <CopyIcon className="size-4" /> Copy name
                      </ContextMenuItem>
                      <ContextMenuSeparator />
                      <ContextMenuItem disabled variant="destructive">
                        <UnplugIcon className="size-4" /> Disconnect agent
                      </ContextMenuItem>
                    </ContextMenuContent>
                  </ContextMenu>
                </SidebarMenuItem>
              ))}
            </SidebarMenu>
          </SidebarGroupContent>
        </SidebarGroup>

        <SidebarGroup>
          <SidebarGroupLabel>Humans</SidebarGroupLabel>
          <SidebarGroupAction
            title="Invite a human"
            onClick={() => setInviteOpen(true)}
          >
            <PlusIcon />
          </SidebarGroupAction>
          <SidebarGroupContent>
            <SidebarMenu>
              {humans.map((human) => (
                <SidebarMenuItem key={human.id}>
                  <ContextMenu>
                    <ContextMenuTrigger asChild>
                      <SidebarMenuButton className="h-9">
                        <MemberAvatar member={human} className="size-6" />
                        <span className="flex-1 truncate">{human.name}</span>
                        <PresenceDot member={human} />
                      </SidebarMenuButton>
                    </ContextMenuTrigger>
                    <ContextMenuContent className="w-52">
                      <ContextMenuItem
                        onSelect={() => setComposerInsert(`@${human.name} `)}
                      >
                        <AtSignIcon className="size-4" /> Mention
                      </ContextMenuItem>
                      <ContextMenuItem
                        onSelect={() =>
                          void navigator.clipboard.writeText(human.name)
                        }
                      >
                        <CopyIcon className="size-4" /> Copy name
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
              <span>Invite people or agents</span>
            </SidebarMenuButton>
          </SidebarMenuItem>
        </SidebarMenu>
      </SidebarFooter>
    </Sidebar>
  );
}
