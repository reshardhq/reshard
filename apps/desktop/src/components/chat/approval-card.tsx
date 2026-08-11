import { useState } from "react";
import type { Approval } from "@agentchat/shared";
import { CheckIcon, CircleAlertIcon, TerminalIcon, XIcon } from "@/components/ui/icons";
import { Button } from "@/components/ui/button";
import { useChat } from "@/lib/use-chat";
import { cn } from "@/lib/utils";

const TERMINAL_LABEL: Record<Exclude<Approval["state"], "pending">, string> = {
  allowed: "Allowed once",
  denied: "Denied",
  expired: "Expired — nothing ran",
  cancelled: "Cancelled — nothing ran",
};

export function ApprovalCard({ approval }: { approval: Approval }) {
  const resolveApproval = useChat((state) => state.resolveApproval);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const pending = approval.state === "pending";

  async function decide(decision: "allowOnce" | "deny") {
    if (!pending || busy) return;
    setBusy(true);
    setError(null);
    try {
      await resolveApproval(approval.id, decision);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Could not resolve approval");
    } finally {
      setBusy(false);
    }
  }

  return (
    <div
      className={cn(
        "mt-1 max-w-md rounded-lg border p-3",
        !pending
          ? "border-border bg-muted/30"
          : "border-amber-500/30 bg-amber-500/5",
      )}
    >
      <div className="flex items-center gap-2 text-xs font-medium uppercase tracking-wide text-muted-foreground">
        <CircleAlertIcon
          className={cn("size-3.5", pending && "text-amber-400")}
        />
        Tool approval · {approval.provider}
      </div>
      <p className="mt-2 text-sm font-medium">{approval.display.summary}</p>
      <div className="mt-2 space-y-1 rounded-md border border-border/70 bg-background/60 p-2 text-xs">
        <div className="flex items-center gap-2">
          <TerminalIcon className="size-3.5 text-muted-foreground" />
          <span className="font-medium">{approval.tool}</span>
          {approval.display.target && (
            <span className="truncate text-muted-foreground">{approval.display.target}</span>
          )}
        </div>
        {approval.display.project && (
          <div className="truncate text-muted-foreground">Project: {approval.display.project}</div>
        )}
        {approval.display.command && (
          <pre className="overflow-x-auto whitespace-pre-wrap break-all font-mono text-[11px] text-foreground/80">
            {approval.display.command}
          </pre>
        )}
      </div>
      <div className="mt-3 flex gap-2">
        {!pending ? (
          <div className="flex items-center gap-1.5 text-sm text-muted-foreground">
            {approval.state === "allowed" ? (
              <CheckIcon className="size-3.5 text-emerald-400" />
            ) : (
              <XIcon className="size-3.5 text-rose-400" />
            )}
            <span className="font-semibold text-foreground">
              {TERMINAL_LABEL[approval.state as Exclude<Approval["state"], "pending">]}
            </span>
          </div>
        ) : (
          <>
            <Button
              type="button"
              size="sm"
              variant="outline"
              disabled={busy}
              onClick={() => void decide("deny")}
            >
              Deny
            </Button>
            <Button
              type="button"
              size="sm"
              disabled={busy}
              onClick={() => void decide("allowOnce")}
            >
              Allow once
            </Button>
          </>
        )}
      </div>
      {error && pending && <p role="alert" className="mt-2 text-xs text-destructive">{error}</p>}
    </div>
  );
}
