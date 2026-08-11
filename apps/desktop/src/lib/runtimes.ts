import { invoke } from "@tauri-apps/api/core";

export type RuntimeAvailability =
  | "ready"
  | "loginRequired"
  | "adapterMissing"
  | "unsupportedVersion"
  | "configInvalid"
  | "notInstalled"
  | "probeFailed";

export interface RuntimeCapabilities {
  nativeAcp: boolean;
  adapterBacked: boolean;
  subscriptionCompatible: boolean;
  resumableSessions: boolean;
  enforceableToolApprovals: boolean;
  cancellation: boolean;
  modelSwitching: boolean;
  maximumParallelism: number;
  executionLocus: "localProcess" | "remoteDaemon" | "externalService";
}

export interface RuntimeInventoryItem {
  id: string;
  label: string;
  version?: string | null;
  availability: RuntimeAvailability;
  auth: "loggedIn" | "loginRequired" | "unknown" | "notApplicable" | "probeFailed";
  adapter: "ready" | "missing" | "notRequired" | "unsupported";
  capabilities: RuntimeCapabilities;
  selected: boolean;
}

export interface RuntimeReport extends Omit<RuntimeInventoryItem, "selected"> {
  binaryPath?: string | null;
  launch?: { command: string; args: string[] } | null;
  diagnostics: Array<{
    level: "info" | "warning" | "error";
    code: string;
    message: string;
    remediation?: string | null;
  }>;
}

export async function discoverLocalRuntimes(refresh = false): Promise<RuntimeReport[]> {
  if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window)) return [];
  return invoke<RuntimeReport[]>("discover_runtimes", { refresh });
}

export function availabilityLabel(value: RuntimeAvailability) {
  switch (value) {
    case "ready":
      return "Ready";
    case "loginRequired":
      return "Login required";
    case "adapterMissing":
      return "Adapter missing";
    case "unsupportedVersion":
      return "Unsupported version";
    case "configInvalid":
      return "Configuration invalid";
    case "notInstalled":
      return "Not installed";
    case "probeFailed":
      return "Probe failed";
  }
}
