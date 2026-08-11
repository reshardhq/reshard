// Best-effort native desktop notifications for owner approvals (Phase 6.4).
//
// The owner sees the approval card inline, but if the window is unfocused a
// gated tool call would silently wait. A native notification surfaces it. This
// is best-effort: it no-ops in the browser dev build and never throws into the
// approval flow.
import type { Approval } from "@agentchat/shared";

function inTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

let permissionResolved = false;
let permissionGranted = false;

async function ensurePermission(): Promise<boolean> {
  if (permissionResolved) return permissionGranted;
  const { isPermissionGranted, requestPermission } = await import(
    "@tauri-apps/plugin-notification"
  );
  permissionGranted = await isPermissionGranted();
  if (!permissionGranted) {
    permissionGranted = (await requestPermission()) === "granted";
  }
  permissionResolved = true;
  return permissionGranted;
}

/** Fire a native notification for a newly requested approval. Never rejects. */
export async function notifyApprovalRequested(approval: Approval): Promise<void> {
  try {
    if (!inTauri()) return;
    if (!(await ensurePermission())) return;
    const { sendNotification } = await import("@tauri-apps/plugin-notification");
    const detail =
      approval.display.command ||
      approval.display.target ||
      approval.display.summary ||
      approval.tool;
    sendNotification({
      title: `Approval needed · ${approval.tool}`,
      body: `${approval.agentId} — ${detail}`,
    });
  } catch {
    // Notifications are best-effort; never break the approval flow.
  }
}
