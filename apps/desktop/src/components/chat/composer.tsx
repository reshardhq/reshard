import { useEffect, useMemo, useRef, useState } from "react";
import { SendIcon } from "@/components/ui/icons";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import { MemberAvatar } from "@/components/member-avatar";
import { useChat } from "@/lib/use-chat";
import { cn } from "@/lib/utils";

export function Composer() {
  const {
    channels,
    activeChannelId,
    members,
    send,
    composerInsert,
    setComposerInsert,
    agentTouch,
  } = useChat();
  const [text, setText] = useState("");
  const [mentionSel, setMentionSel] = useState(0);
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    if (composerInsert == null) return;
    setText((t) => (t ? `${t.trimEnd()} ${composerInsert}` : composerInsert));
    setComposerInsert(null);
    textareaRef.current?.focus();
  }, [composerInsert, setComposerInsert]);

  const channel = channels.find((c) => c.id === activeChannelId);

  const mentionQuery = useMemo(() => {
    const match = text.match(/@([\w-]*)$/);
    return match ? match[1].toLowerCase() : null;
  }, [text]);

  const mentionMatches = useMemo(() => {
    if (mentionQuery == null) return [];
    return members
      .filter((m) => m.name.toLowerCase().startsWith(mentionQuery))
      .slice(0, 5);
  }, [mentionQuery, members]);

  const completeMention = (name: string) => {
    setText(text.replace(/@[\w-]*$/, `@${name} `));
    setMentionSel(0);
    textareaRef.current?.focus();
  };

  const submit = () => {
    if (!text.trim()) return;
    send(text);
    setText("");
    setMentionSel(0);
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (mentionMatches.length > 0) {
      if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        e.preventDefault();
        const delta = e.key === "ArrowDown" ? 1 : -1;
        setMentionSel(
          (mentionSel + delta + mentionMatches.length) % mentionMatches.length,
        );
        return;
      }
      if (e.key === "Tab" || e.key === "Enter") {
        e.preventDefault();
        completeMention(mentionMatches[mentionSel].name);
        return;
      }
      if (e.key === "Escape") {
        setText(text + " ");
        return;
      }
    }
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      submit();
    }
  };

  return (
    <div className="relative border-t px-4 py-3">
      {mentionMatches.length > 0 && (
        <div className="absolute bottom-full left-4 z-10 mb-1 w-64 overflow-hidden rounded-lg border bg-popover shadow-md">
          {mentionMatches.map((member, i) => (
            <button
              key={member.id}
              className={cn(
                "flex w-full items-center gap-2 px-2.5 py-1.5 text-left text-sm",
                i === mentionSel && "bg-muted",
              )}
              onMouseEnter={() => setMentionSel(i)}
              onClick={() => completeMention(member.name)}
            >
              <MemberAvatar member={member} className="size-5" />
              <span className="font-medium">{member.name}</span>
              <span className="ml-auto text-[10px] uppercase text-muted-foreground">
                {member.type}
              </span>
            </button>
          ))}
        </div>
      )}
      <div
        className={cn(
          "flex items-end gap-2",
          agentTouch === "composer" && "agent-touch",
        )}
      >
        <Textarea
          ref={textareaRef}
          value={text}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={onKeyDown}
          placeholder={channel ? `Message ${channel.name}` : "Pick a chat"}
          rows={1}
          className="max-h-40 min-h-10 flex-1 resize-none"
        />
        <Button size="icon" onClick={submit} disabled={!text.trim()}>
          <SendIcon className="size-4 -translate-x-0.5 rotate-45" />
        </Button>
      </div>
    </div>
  );
}
