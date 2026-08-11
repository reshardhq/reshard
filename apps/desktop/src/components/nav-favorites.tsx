import {
  DndContext,
  PointerSensor,
  closestCenter,
  useSensor,
  useSensors,
  type DragEndEvent,
} from "@dnd-kit/core"
import {
  SortableContext,
  useSortable,
  verticalListSortingStrategy,
} from "@dnd-kit/sortable"
import { CSS } from "@dnd-kit/utilities"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import {
  SidebarGroup,
  SidebarGroupAction,
  SidebarGroupLabel,
  SidebarMenu,
  SidebarMenuAction,
  SidebarMenuButton,
  SidebarMenuItem,
  useSidebar,
} from "@/components/ui/sidebar"
import {
  MoreHorizontalIcon,
  LinkIcon,
  BellOffIcon,
  MessageSquareIcon,
  PlusIcon,
} from "@/components/ui/icons"
import type { Channel, Member } from "@agentchat/shared"
import { Badge } from "@/components/ui/badge"
import { AvatarFrame } from "@/components/member-avatar"
import { useChat } from "@/lib/use-chat"
import { cn } from "@/lib/utils"

function ChatAvatar({
  conversation,
  members,
}: {
  conversation: Channel
  members: Member[]
}) {
  // one avatar per chat: DM = the member's, group = seeded by group name
  const seed =
    conversation.kind === "dm"
      ? (members.find(
          (m) => conversation.memberIds.includes(m.id) && m.type === "agent",
        )?.name ?? conversation.name)
      : (conversation.avatarSeed ?? conversation.name)
  return <AvatarFrame name={seed} className="size-5" />
}

function SortableChat({ conversation }: { conversation: Channel }) {
  const { isMobile } = useSidebar()
  const { activeChannelId, selectChannel, agentTouch, members, unread } =
    useChat()
  const count = unread[conversation.id] ?? 0
  const {
    attributes,
    listeners,
    setNodeRef,
    transform,
    transition,
    isDragging,
  } = useSortable({ id: conversation.id })

  const dmMember =
    conversation.kind === "dm"
      ? members.find(
          (m) => conversation.memberIds.includes(m.id) && m.type === "agent",
        )
      : undefined

  return (
    <SidebarMenuItem
      ref={setNodeRef}
      style={{
        transform: CSS.Transform.toString(transform),
        transition,
      }}
      className={cn(isDragging && "z-10 opacity-80")}
      {...attributes}
      {...listeners}
    >
      <SidebarMenuButton
        title={conversation.topic ?? conversation.name}
        isActive={conversation.id === activeChannelId}
        onClick={() => void selectChannel(conversation.id)}
        className={cn(
          "h-9",
          agentTouch === `channel:${conversation.id}` && "agent-touch",
        )}
      >
        <ChatAvatar conversation={conversation} members={members} />
        <span
          className={cn(
            "flex-1 truncate capitalize",
            dmMember?.presence === "offline" && "text-muted-foreground",
          )}
        >
          {conversation.name}
        </span>
        {count > 0 && (
          <Badge className="h-4 min-w-4 rounded-full px-1 font-mono text-[10px] tabular-nums">
            {count}
          </Badge>
        )}
      </SidebarMenuButton>
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <SidebarMenuAction showOnHover className="aria-expanded:bg-muted">
            <MoreHorizontalIcon />
            <span className="sr-only">More</span>
          </SidebarMenuAction>
        </DropdownMenuTrigger>
        <DropdownMenuContent
          className="w-56 rounded-lg"
          side={isMobile ? "bottom" : "right"}
          align={isMobile ? "end" : "start"}
        >
          <DropdownMenuItem onSelect={() => void selectChannel(conversation.id)}>
            <MessageSquareIcon className="text-muted-foreground" />
            <span>Open chat</span>
          </DropdownMenuItem>
          <DropdownMenuItem
            onSelect={() =>
              void navigator.clipboard.writeText(conversation.name)
            }
          >
            <LinkIcon className="text-muted-foreground" />
            <span>Copy name</span>
          </DropdownMenuItem>
          <DropdownMenuSeparator />
          <DropdownMenuItem disabled>
            <BellOffIcon className="text-muted-foreground" />
            <span>Mute chat</span>
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
    </SidebarMenuItem>
  )
}

export function NavFavorites() {
  const { channels, setNewChatOpen, reorderChannels } = useChat()
  const sensors = useSensors(
    // small activation distance keeps plain clicks working
    useSensor(PointerSensor, { activationConstraint: { distance: 5 } }),
  )

  const onDragEnd = ({ active, over }: DragEndEvent) => {
    if (over && active.id !== over.id) {
      reorderChannels(String(active.id), String(over.id))
    }
  }

  return (
    <SidebarGroup className="pt-0 group-data-[collapsible=icon]:hidden">
      <SidebarGroupLabel>Chats</SidebarGroupLabel>
      <SidebarGroupAction
        title="Start a chat"
        onClick={() => setNewChatOpen(true)}
        className="top-1.5"
      >
        <PlusIcon />
      </SidebarGroupAction>
      <SidebarMenu>
        <DndContext
          sensors={sensors}
          collisionDetection={closestCenter}
          onDragEnd={onDragEnd}
        >
          <SortableContext
            items={channels.map((c) => c.id)}
            strategy={verticalListSortingStrategy}
          >
            {channels.map((conversation) => (
              <SortableChat key={conversation.id} conversation={conversation} />
            ))}
          </SortableContext>
        </DndContext>
      </SidebarMenu>
    </SidebarGroup>
  )
}
