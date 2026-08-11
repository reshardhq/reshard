"use client";

import { useState } from "react";
import { Check, Copy } from "@/components/ui/icons";
import { Button } from "@/components/ui/button";

export function CopyCommand({ command }: { command: string }) {
  const [copied, setCopied] = useState(false);

  async function copy() {
    await navigator.clipboard.writeText(command);
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1400);
  }

  return (
    <div className="flex min-w-0 items-center border border-white/12 bg-black/35">
      <span className="px-4 font-mono text-xs text-[#6f8fff]">$</span>
      <code className="min-w-0 flex-1 overflow-x-auto whitespace-nowrap py-3.5 font-mono text-[11px] text-white/65 [scrollbar-width:none]">{command}</code>
      <Button type="button" size="sm" variant="ghost" onClick={copy} className="mx-1 px-3 font-mono uppercase tracking-[0.12em]">
        {copied ? <Check /> : <Copy />}{copied ? "Copied" : "Copy"}
      </Button>
    </div>
  );
}
