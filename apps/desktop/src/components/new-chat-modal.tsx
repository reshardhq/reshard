import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import { useChat } from "@/lib/use-chat";

/**
 * Start a private 1:1 chat. It begins with its owner, then one agent is
 * connected during onboarding.
 */
export function NewChatModal() {
  const { newChatOpen, setNewChatOpen, createChannel, members } = useChat();
  const agents = members.filter((member) => member.type === "agent");
  const [name, setName] = useState("");
  const [topic, setTopic] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (newChatOpen) {
      setName("");
      setTopic("");
      setError(null);
    }
  }, [newChatOpen]);

  const submit = async () => {
    if (!name.trim() || busy) return;
    setBusy(true);
    setError(null);
    try {
      await createChannel(name.trim(), topic.trim() || undefined, agents[0]?.id);
      setNewChatOpen(false);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog open={newChatOpen} onOpenChange={setNewChatOpen}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>Start a chat</DialogTitle>
          <DialogDescription>
            Give your agent chat a name. Your local agent will be attached automatically.
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-4">
          <div className="space-y-2">
            <Label htmlFor="group-name">Name</Label>
            <Input
              id="chat-name"
              value={name}
              autoFocus
              placeholder="Phone Repairs"
              onChange={(e) => setName(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") void submit();
              }}
            />
          </div>
          <div className="space-y-2">
            <Label htmlFor="chat-topic">
              Description{" "}
              <span className="text-muted-foreground">(optional)</span>
            </Label>
            <Textarea
              id="chat-topic"
              value={topic}
              rows={2}
              placeholder="cracked screens and quotes"
              onChange={(e) => setTopic(e.target.value)}
            />
          </div>
          {error && <p className="text-xs text-destructive">{error}</p>}
        </div>

        <DialogFooter>
          <Button variant="ghost" onClick={() => setNewChatOpen(false)}>
            Cancel
          </Button>
          <Button onClick={submit} disabled={!name.trim() || busy}>
            {busy ? "Creating…" : "Create chat"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
