import { useEffect, useRef, useState, type CSSProperties } from "react";
import {
  SidebarInset,
  SidebarProvider,
  useSidebar,
} from "@/components/ui/sidebar";
import { cn } from "@/lib/utils";
import { SidebarLeft } from "@/components/sidebar-left";
import { SidebarRight } from "@/components/sidebar-right";
import { ChatView } from "@/components/chat/chat-view";
import { InviteModal } from "@/components/invite-modal";
import { NewChatModal } from "@/components/new-chat-modal";
import { PrefsModal } from "@/components/prefs-modal";
import { CaddyPanel } from "@/components/caddy-panel";
import { TitleBar } from "@/components/title-bar";
import { StatusBar } from "@/components/status-bar";
import { CommandPalette } from "@/components/command-palette";
import { useChat } from "@/lib/use-chat";
import { AuthPage } from "@/components/auth-page";

function Shortcuts() {
  const { toggleSidebar } = useSidebar();

  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (!e.metaKey && !e.ctrlKey) return;
      if (
        import.meta.env.DEV &&
        e.shiftKey &&
        e.key.toLowerCase() === "r"
      ) {
        e.preventDefault();
        window.location.reload();
        return;
      }
      if (e.key >= "1" && e.key <= "9") {
        const { channels, selectChannel } = useChat.getState();
        const channel = channels[Number(e.key) - 1];
        if (channel) {
          e.preventDefault();
          void selectChannel(channel.id);
        }
        return;
      }
      if (e.key === "j") {
        e.preventDefault();
        const { caddyOpen, setCaddyOpen } = useChat.getState();
        setCaddyOpen(!caddyOpen);
        return;
      }
      if (e.key === ",") {
        e.preventDefault();
        const { prefsOpen, setPrefsOpen } = useChat.getState();
        setPrefsOpen(!prefsOpen);
        return;
      }
      if (e.key === "\\") {
        e.preventDefault();
        toggleSidebar();
        return;
      }
      if (e.key === "i") {
        e.preventDefault();
        const { activityOpen, setActivityOpen } = useChat.getState();
        setActivityOpen(!activityOpen);
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [toggleSidebar]);

  return null;
}

const clamp = (v: number, lo: number, hi: number) =>
  Math.min(hi, Math.max(lo, v));

/** sidebars stay between 180px and 35% of the window */
const clampSidebar = (v: number) =>
  clamp(v, 180, Math.round(window.innerWidth * 0.35));

function storedWidth(key: string, fallback: number) {
  const v = Number(localStorage.getItem(key));
  return v > 0 ? v : fallback;
}

function ResizeHandle({
  side,
  offset,
  onDrag,
  onDragging,
}: {
  side: "left" | "right";
  offset: number;
  onDrag: (clientX: number) => void;
  onDragging: (dragging: boolean) => void;
}) {
  return (
    <div
      className="fixed bottom-8 top-9.5 z-40 w-1.5 cursor-col-resize"
      style={side === "left" ? { left: offset - 3 } : { right: offset - 3 }}
      onPointerDown={(e) => {
        e.preventDefault();
        onDragging(true);
        const move = (ev: PointerEvent) => onDrag(ev.clientX);
        const up = () => {
          onDragging(false);
          window.removeEventListener("pointermove", move);
          window.removeEventListener("pointerup", up);
        };
        window.addEventListener("pointermove", move);
        window.addEventListener("pointerup", up);
      }}
    />
  );
}

function LeftResizeHandle({
  width,
  onDrag,
  onDragging,
}: {
  width: number;
  onDrag: (x: number) => void;
  onDragging: (d: boolean) => void;
}) {
  const { state } = useSidebar();
  if (state !== "expanded") return null;
  return (
    <ResizeHandle
      side="left"
      offset={width}
      onDrag={onDrag}
      onDragging={onDragging}
    />
  );
}

export default function App() {
  const init = useChat((s) => s.init);
  const authed = useChat((s) => s.authed);
  const authReady = useChat((s) => s.authReady);
  const initAuth = useChat((s) => s.initAuth);
  const refreshMachines = useChat((s) => s.refreshMachines);
  const rightSidebarOpen = useChat((s) => s.rightSidebarOpen);
  const [leftWidth, setLeftWidth] = useState(() =>
    storedWidth("agentchat.leftWidth", 256),
  );
  const [rightWidth, setRightWidth] = useState(() =>
    storedWidth("agentchat.rightWidth", 288),
  );
  const [resizing, setResizing] = useState(false);
  const saveTimer = useRef<number | undefined>(undefined);

  useEffect(() => {
    window.clearTimeout(saveTimer.current);
    saveTimer.current = window.setTimeout(() => {
      localStorage.setItem("agentchat.leftWidth", String(leftWidth));
      localStorage.setItem("agentchat.rightWidth", String(rightWidth));
    }, 300);
  }, [leftWidth, rightWidth]);

  useEffect(() => {
    // window shrinks re-clamp both sidebars
    const onResize = () => {
      setLeftWidth((w) => clampSidebar(w));
      setRightWidth((w) => clampSidebar(w));
    };
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  }, []);

  useEffect(() => {
    void initAuth();
  }, [initAuth]);

  useEffect(() => {
    if (authReady && authed) void init();
  }, [authReady, authed, init]);

  useEffect(() => {
    if (!authReady || !authed) return;
    void refreshMachines();
    const timer = window.setInterval(() => void refreshMachines(), 5_000);
    return () => window.clearInterval(timer);
  }, [authReady, authed, refreshMachines]);

  useEffect(() => {
    // native webview context menu never shows; app menus own right-click
    const onContextMenu = (e: MouseEvent) => e.preventDefault();
    window.addEventListener("contextmenu", onContextMenu);
    return () => window.removeEventListener("contextmenu", onContextMenu);
  }, []);

  if (!authReady) return <div className="h-svh bg-background" />;
  if (!authed) return <AuthPage />;

  return (
    <SidebarProvider
      style={{ "--sidebar-width": `${leftWidth}px` } as CSSProperties}
    >
      <Shortcuts />
      <TitleBar />
      <SidebarLeft />
      <LeftResizeHandle
        width={leftWidth}
        onDrag={(x) => setLeftWidth(clampSidebar(x))}
        onDragging={setResizing}
      />
      <SidebarInset className="h-svh min-h-0 overflow-hidden pb-8 pt-9.5">
        <ChatView />
      </SidebarInset>
      <div
        className={cn(
          "shrink-0 overflow-hidden",
          !resizing && "transition-[width] duration-200 ease-linear",
        )}
        style={{ width: rightSidebarOpen ? rightWidth : 0 }}
      >
        <SidebarRight
          className="pb-8 pt-9.5"
          style={{ "--sidebar-width": `${rightWidth}px` } as CSSProperties}
        />
      </div>
      {rightSidebarOpen && (
        <ResizeHandle
          side="right"
          offset={rightWidth}
          onDrag={(x) => setRightWidth(clampSidebar(window.innerWidth - x))}
          onDragging={setResizing}
        />
      )}
      <StatusBar />
      <CommandPalette />
      <CaddyPanel />
      <PrefsModal />
      <InviteModal />
      <NewChatModal />
    </SidebarProvider>
  );
}
