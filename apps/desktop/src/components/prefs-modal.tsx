import { useEffect, useState } from "react";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import { Separator } from "@/components/ui/separator";
import { useChat } from "@/lib/use-chat";
import * as auth from "@/lib/auth";
import {
  availabilityLabel,
  discoverLocalRuntimes,
  type RuntimeReport,
} from "@/lib/runtimes";

function Row({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: string;
  children: React.ReactNode;
}) {
  return (
    <div className="flex items-center justify-between gap-4 py-2">
      <div>
        <p className="text-sm font-medium">{label}</p>
        {hint && <p className="text-xs text-muted-foreground">{hint}</p>}
      </div>
      {children}
    </div>
  );
}

export function PrefsModal() {
  const { prefsOpen, setPrefsOpen, refreshMachines } = useChat();
  const [machines, setMachines] = useState<auth.Machine[]>([]);
  const [deviceCode, setDeviceCode] = useState("");
  const [machineError, setMachineError] = useState<string | null>(null);
  const [machineNotice, setMachineNotice] = useState<string | null>(null);
  const [machineBusy, setMachineBusy] = useState(false);
  const [localRuntimes, setLocalRuntimes] = useState<RuntimeReport[]>([]);
  const [runtimeBusy, setRuntimeBusy] = useState(false);

  async function loadMachines() {
    const status = await auth.machineStatus();
    setMachines(status.machines);
    await refreshMachines();
  }

  async function loadLocalRuntimes(refresh = false) {
    setRuntimeBusy(true);
    try {
      setLocalRuntimes(await discoverLocalRuntimes(refresh));
    } finally {
      setRuntimeBusy(false);
    }
  }

  useEffect(() => {
    if (!prefsOpen) return;
    void loadMachines().catch((error) => {
      setMachineError(error instanceof Error ? error.message : "Could not load machines.");
    });
    void loadLocalRuntimes().catch((error) => {
      setMachineError(error instanceof Error ? error.message : "Could not discover runtimes.");
    });
    // Opening the modal is the refresh boundary. Keeping the callback out of
    // the dependency list avoids reloading after unrelated store updates.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [prefsOpen]);

  async function approveMachine() {
    if (!deviceCode.trim() || machineBusy) return;
    setMachineBusy(true);
    setMachineError(null);
    setMachineNotice(null);
    try {
      await auth.approveDevice(deviceCode.trim());
      setDeviceCode("");
      setMachineNotice("Machine approved. It will appear after the CLI finishes signing in.");
      window.setTimeout(() => void loadMachines(), 2_500);
    } catch (error) {
      setMachineError(error instanceof Error ? error.message : "Could not approve that code.");
    } finally {
      setMachineBusy(false);
    }
  }

  async function revokeMachine(machine: auth.Machine) {
    if (machineBusy) return;
    setMachineBusy(true);
    setMachineError(null);
    setMachineNotice(null);
    try {
      await auth.revokeMachine(machine.id);
      await loadMachines();
      setMachineNotice(`${machine.name} was revoked.`);
    } catch (error) {
      setMachineError(error instanceof Error ? error.message : "Could not revoke that machine.");
    } finally {
      setMachineBusy(false);
    }
  }

  return (
    <Dialog open={prefsOpen} onOpenChange={setPrefsOpen}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>Preferences</DialogTitle>
          <DialogDescription>Workspace and app settings.</DialogDescription>
        </DialogHeader>
        <div className="flex flex-col">
          <Row label="Workspace name">
            <Input defaultValue="t31k's workspace" className="h-8 w-44 text-sm" />
          </Row>
          <Separator />
          <Row label="Theme" hint="Light theme lands with the design pass">
            <span className="rounded-md border px-2 py-1 font-mono text-xs text-muted-foreground">
              dark
            </span>
          </Row>
          <Separator />
          <Row label="Relay" hint="Hosted relay arrives in phase 2">
            <span className="rounded-md border px-2 py-1 font-mono text-xs text-muted-foreground">
              mock
            </span>
          </Row>
          <Separator />
          <Row label="Notifications" hint="Push requires the hosted relay">
            <span className="rounded-md border px-2 py-1 font-mono text-xs text-muted-foreground">
              off
            </span>
          </Row>
          <Separator />
          <div className="py-3">
            <div className="flex items-center justify-between">
              <div>
                <p className="text-sm font-medium">Runtimes on this machine</p>
                <p className="text-xs text-muted-foreground">
                  The same trusted discovery engine used by the Rebeam CLI.
                </p>
              </div>
              <Button
                size="sm"
                variant="outline"
                disabled={runtimeBusy}
                onClick={() => void loadLocalRuntimes(true)}
              >
                {runtimeBusy ? "Checking…" : "Refresh"}
              </Button>
            </div>
            <div className="mt-3 grid gap-2">
              {localRuntimes.length === 0 ? (
                <p className="text-xs text-muted-foreground">
                  Runtime discovery is available in the native Rebeam app.
                </p>
              ) : (
                localRuntimes.map((runtime) => (
                  <div key={runtime.id} className="rounded-md border px-2.5 py-2">
                    <div className="flex items-center justify-between gap-3">
                      <p className="text-sm">{runtime.label}</p>
                      <span
                        className={
                          runtime.availability === "ready"
                            ? "text-xs text-emerald-400"
                            : "text-xs text-muted-foreground"
                        }
                      >
                        {availabilityLabel(runtime.availability)}
                      </span>
                    </div>
                    {runtime.binaryPath && (
                      <p className="truncate font-mono text-[11px] text-muted-foreground">
                        {runtime.binaryPath}
                      </p>
                    )}
                    {runtime.diagnostics[0] && (
                      <p className="mt-1 text-xs text-muted-foreground">
                        {runtime.diagnostics[0].message}
                      </p>
                    )}
                  </div>
                ))
              )}
            </div>
          </div>
          <Separator />
          <div className="py-3">
            <p className="text-sm font-medium">Machines</p>
            <p className="mb-3 text-xs text-muted-foreground">
              Approve a CLI login code or revoke a machine that should no longer run your agents.
            </p>
            <div className="flex gap-2">
              <Input
                value={deviceCode}
                onChange={(event) => setDeviceCode(event.target.value.toUpperCase())}
                onKeyDown={(event) => {
                  if (event.key === "Enter") void approveMachine();
                }}
                placeholder="RBM-XXXX-XXXX-XXXX"
                className="h-8 font-mono text-xs"
              />
              <Button
                size="sm"
                disabled={!deviceCode.trim() || machineBusy}
                onClick={() => void approveMachine()}
              >
                Approve
              </Button>
            </div>
            {machineError && <p className="mt-2 text-xs text-destructive">{machineError}</p>}
            {machineNotice && <p className="mt-2 text-xs text-emerald-400">{machineNotice}</p>}
            <div className="mt-3 flex flex-col gap-2">
              {machines.length === 0 ? (
                <p className="text-xs text-muted-foreground">No CLI machines connected.</p>
              ) : (
                machines.map((machine) => (
                  <div key={machine.id} className="flex items-center justify-between rounded-md border px-2.5 py-2">
                    <div className="min-w-0">
                      <p className="truncate text-sm">{machine.name}</p>
                      <p className="text-xs text-muted-foreground">
                        {machine.online ? "Online" : "Offline"} · {machine.id}
                      </p>
                      {machine.runtimes.length > 0 && (
                        <p className="mt-1 text-xs text-muted-foreground">
                          {machine.runtimes
                            .map(
                              (runtime) =>
                                `${runtime.label}: ${availabilityLabel(runtime.availability)}`,
                            )
                            .join(" · ")}
                        </p>
                      )}
                    </div>
                    <Button
                      size="sm"
                      variant="outline"
                      disabled={machineBusy}
                      onClick={() => void revokeMachine(machine)}
                    >
                      Revoke
                    </Button>
                  </div>
                ))
              )}
            </div>
          </div>
        </div>
      </DialogContent>
    </Dialog>
  );
}
