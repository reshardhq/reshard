import { useEffect, useState } from "react";
import { DitherBackdrop } from "@/components/dither-backdrop";
import { AvatarFrame } from "@/components/member-avatar";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Field, FieldLabel } from "@/components/ui/field";
import { CheckIcon, CopyIcon, RotateCcwIcon } from "@/components/ui/icons";
import { Input } from "@/components/ui/input";
import { createPairing, machineStatus } from "@/lib/auth";
import { useChat } from "@/lib/use-chat";
import { cn } from "@/lib/utils";

const INSTALL_CMD = "curl -fsSL https://reshard.dev/install.sh | sh";

// A dither avatar is deterministic from its seed, so a fresh seed is a fresh
// avatar.
function randomSeed() {
  return Math.random().toString(36).slice(2, 10);
}

// 8-char code shown grouped as XXXX-XXXX; stored/typed without the dash works too.
function formatCode(code: string) {
  return code.length === 8 ? `${code.slice(0, 4)}-${code.slice(4)}` : code;
}

function useCopy() {
  const [copied, setCopied] = useState(false);
  const copy = (text: string) =>
    void navigator.clipboard.writeText(text).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    });
  return { copied, copy };
}

function CommandRow({ label, command }: { label: string; command: string }) {
  const { copied, copy } = useCopy();
  return (
    <div className="flex flex-col gap-1.5">
      <span className="text-sm font-medium">{label}</span>
      <div className="relative rounded-md border bg-muted/40 px-3 py-2.5 pr-10 font-mono text-xs text-muted-foreground">
        <pre className="overflow-x-auto whitespace-pre">{command}</pre>
        <Button
          size="icon"
          variant="ghost"
          className="absolute right-1 top-1 size-7"
          onClick={() => copy(command)}
        >
          {copied ? (
            <CheckIcon className="size-3.5 text-emerald-400" />
          ) : (
            <CopyIcon className="size-3.5" />
          )}
        </Button>
      </div>
    </div>
  );
}

// Fixed height so swapping the title/subtitle never shifts the card layout.
function StepHeader({ title, subtitle }: { title: string; subtitle: string }) {
  return (
    <div className="flex h-[60px] flex-col items-center gap-1.5 text-center">
      <h1 className="text-2xl font-bold tracking-tight">{title}</h1>
      <p className="text-balance text-sm text-muted-foreground">{subtitle}</p>
    </div>
  );
}

export function Onboarding({ onDone }: { onDone: () => void }) {
  const claimUsername = useChat((s) => s.claimUsername);
  const [step, setStep] = useState<0 | 1 | 2>(0);
  const [username, setUsername] = useState("");
  const [name, setName] = useState("");
  const [seed, setSeed] = useState(randomSeed());
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [code, setCode] = useState<string | null>(null);
  const [status, setStatus] = useState<"waiting" | "online">("waiting");
  const { copied, copy } = useCopy();

  const cleanUsername = username.trim().toLowerCase();
  const usernameValid =
    cleanUsername.length >= 3 &&
    cleanUsername.length <= 32 &&
    /^[a-z0-9_-]+$/.test(cleanUsername);

  async function claim() {
    setBusy(true);
    setError(null);
    try {
      await claimUsername(cleanUsername, name.trim() || cleanUsername);
      setStep(1);
    } catch (e) {
      setError(e instanceof Error ? e.message : "Could not claim that username.");
    } finally {
      setBusy(false);
    }
  }

  // Mint a real pairing code when we reach the pairing step (needs the session).
  useEffect(() => {
    if (step !== 2 || code) return;
    let alive = true;
    createPairing()
      .then((c) => alive && setCode(c))
      .catch(() => {});
    return () => {
      alive = false;
    };
  }, [step, code]);

  // Poll for a connected machine while on the pairing step.
  useEffect(() => {
    if (step !== 2) return;
    let alive = true;
    const tick = async () => {
      try {
        const s = await machineStatus();
        if (alive && s.count > 0) setStatus("online");
      } catch {
        /* keep waiting */
      }
    };
    void tick();
    const id = setInterval(() => void tick(), 3000);
    return () => {
      alive = false;
      clearInterval(id);
    };
  }, [step]);

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center overflow-hidden bg-background p-6">
      <div className="absolute inset-0">
        <DitherBackdrop variant="band" pixel={3} opacity={0.07} />
      </div>
      <div data-tauri-drag-region className="fixed inset-x-0 top-0 z-10 h-10" />

      <div className="w-full max-w-md">
        {/* step indicator */}
        <div className="mb-6 flex items-center justify-center gap-2">
          {[0, 1, 2].map((i) => (
            <span
              key={i}
              className={cn(
                "h-0.5 w-8 rounded-full transition-colors",
                i <= step ? "bg-foreground" : "bg-foreground/15",
              )}
            />
          ))}
        </div>

        <Card className="relative flex min-h-[420px] flex-col gap-0 p-8 tracking-tight">
          {step === 0 && (
            <>
              <StepHeader
                title="Claim your handle"
                subtitle="Your unique username on Reshard — and how people see you."
              />
              <div className="mt-8 flex flex-1 flex-col gap-5">
                <div className="flex flex-col items-center gap-3">
                  <AvatarFrame name={seed} className="size-20 rounded-md" />
                  <Button
                    variant="ghost"
                    size="sm"
                    className="gap-1.5 text-muted-foreground"
                    onClick={() => setSeed(randomSeed())}
                  >
                    <RotateCcwIcon className="size-3.5" /> Shuffle avatar
                  </Button>
                </div>
                <Field>
                  <FieldLabel htmlFor="username">Username</FieldLabel>
                  <Input
                    id="username"
                    value={username}
                    onChange={(e) => setUsername(e.target.value)}
                    placeholder="username"
                    autoFocus
                    autoComplete="off"
                    autoCapitalize="none"
                  />
                  {username && !usernameValid && (
                    <span className="text-xs text-muted-foreground">
                      3–32 characters: letters, numbers, _ or -
                    </span>
                  )}
                </Field>
                <Field>
                  <FieldLabel htmlFor="name">Display name</FieldLabel>
                  <Input
                    id="name"
                    value={name}
                    onChange={(e) => setName(e.target.value)}
                    placeholder="Your name (optional)"
                    onKeyDown={(e) => {
                      if (e.key === "Enter" && usernameValid && !busy) void claim();
                    }}
                  />
                </Field>
                {error && (
                  <span className="text-xs text-destructive">{error}</span>
                )}
                <Button
                  className="mt-auto"
                  disabled={!usernameValid || busy}
                  onClick={() => void claim()}
                >
                  {busy ? "Claiming…" : "Continue"}
                </Button>
              </div>
            </>
          )}

          {step === 1 && (
            <>
              <StepHeader
                title="Bring your agents online"
                subtitle="Run this on the machine where your agents live — your Mac or a VPS."
              />
              <div className="mt-8 flex flex-1 flex-col gap-4">
                <CommandRow label="Install the Reshard CLI" command={INSTALL_CMD} />
                <p className="text-sm leading-6 text-muted-foreground">
                  Then run{" "}
                  <code className="rounded bg-muted px-1.5 py-0.5 font-mono text-xs text-foreground">
                    reshard setup
                  </code>{" "}
                  — it will ask for the pairing code on the next step.
                </p>
                <Button className="mt-auto" onClick={() => setStep(2)}>
                  Next
                </Button>
              </div>
            </>
          )}

          {step === 2 && (
            <>
              <StepHeader
                title="Pair your machine"
                subtitle="Enter this code when reshard setup asks — on any terminal."
              />
              <div className="mt-8 flex flex-1 flex-col items-center gap-6">
                <div className="relative w-full rounded-lg border bg-muted/40 py-5 pr-12 text-center font-mono text-2xl font-semibold tracking-[0.35em]">
                  {code ? formatCode(code) : "········"}
                  <Button
                    size="icon"
                    variant="ghost"
                    className="absolute right-2 top-1/2 size-8 -translate-y-1/2"
                    disabled={!code}
                    onClick={() => code && copy(code)}
                  >
                    {copied ? (
                      <CheckIcon className="size-4 text-emerald-400" />
                    ) : (
                      <CopyIcon className="size-4" />
                    )}
                  </Button>
                </div>

                <div className="flex items-center gap-2 text-sm">
                  <span
                    className={cn(
                      "size-1.5 rounded-full",
                      status === "online"
                        ? "bg-emerald-400"
                        : "animate-pulse bg-amber-400",
                    )}
                  />
                  <span className="text-muted-foreground">
                    {status === "online"
                      ? "Machine connected"
                      : "Waiting for your machine…"}
                  </span>
                </div>

                <Button className="mt-auto w-full" onClick={onDone}>
                  {status === "online" ? "Enter Reshard" : "Skip for now"}
                </Button>
              </div>
            </>
          )}
        </Card>
      </div>
    </div>
  );
}
