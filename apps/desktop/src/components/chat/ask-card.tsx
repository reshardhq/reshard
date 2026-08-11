import type { Message } from "@agentchat/shared";
import { CheckIcon, MessageCircleQuestionIcon } from "@/components/ui/icons";
import { Button } from "@/components/ui/button";
import { useChat } from "@/lib/use-chat";

/** An ordinary chat question. It has no authority over local tools. */
export function AskCard({ message }: { message: Message }) {
  const resolveAsk = useChat((state) => state.resolveAsk);
  const resolved = message.resolvedOption != null;

  return (
    <div className="mt-1 max-w-md rounded-lg border border-border bg-muted/20 p-3">
      <div className="flex items-center gap-2 text-xs font-medium uppercase tracking-wide text-muted-foreground">
        <MessageCircleQuestionIcon className="size-3.5" />
        Question
      </div>
      <p className="mt-2 text-sm font-medium">{message.text}</p>
      <div className="mt-3 flex flex-wrap gap-2">
        {resolved ? (
          <div className="flex items-center gap-1.5 text-sm text-muted-foreground">
            <CheckIcon className="size-3.5 text-emerald-400" />
            Answered <span className="font-semibold text-foreground">{message.resolvedOption}</span>
          </div>
        ) : (
          message.options?.map((option) => (
            <Button
              key={option}
              type="button"
              size="sm"
              variant="outline"
              onClick={() => void resolveAsk(message.id, option)}
            >
              {option}
            </Button>
          ))
        )}
      </div>
    </div>
  );
}
