import { invoke } from "@tauri-apps/api/core";
import type { RuntimeInventoryItem } from "./runtimes";

const RELAY = import.meta.env.VITE_RELAY ?? "http://127.0.0.1:8787";
const BROWSER_SESSION_KEY = "rebeam.authSession";

export interface AuthUser {
  id: string;
  email: string;
  name: string;
}

interface AuthSession {
  token: string;
  refreshToken: string;
  expiresAt: number;
  user: AuthUser;
}

export interface Machine {
  id: string;
  name: string;
  createdAt: number;
  lastSeen?: number | null;
  online: boolean;
  runtimes: RuntimeInventoryItem[];
  runtimeUpdatedAt?: number | null;
}

export interface MachineStatus {
  count: number;
  online: number;
  machines: Machine[];
}

let session: AuthSession | null | undefined;
let user: AuthUser | null = null;
let pendingLoad: Promise<AuthSession | null> | null = null;
let pendingRefresh: Promise<AuthSession | null> | null = null;

function isTauri() {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

function validSession(value: unknown): value is AuthSession {
  if (!value || typeof value !== "object") return false;
  const candidate = value as Partial<AuthSession>;
  return (
    typeof candidate.token === "string" &&
    typeof candidate.refreshToken === "string" &&
    typeof candidate.expiresAt === "number" &&
    !!candidate.user &&
    typeof candidate.user.id === "string"
  );
}

async function loadSession(): Promise<AuthSession | null> {
  if (session !== undefined) return session;
  if (!pendingLoad) {
    pendingLoad = (async () => {
      let encoded: string | null = null;
      if (isTauri()) {
        encoded = await invoke<string | null>("load_auth_session");
      } else {
        encoded = localStorage.getItem(BROWSER_SESSION_KEY);
      }
      if (!encoded) return null;
      try {
        const parsed: unknown = JSON.parse(encoded);
        return validSession(parsed) ? parsed : null;
      } catch {
        return null;
      }
    })();
  }
  session = await pendingLoad;
  pendingLoad = null;
  user = session?.user ?? null;
  return session;
}

async function persistSession(next: AuthSession | null) {
  session = next;
  user = next?.user ?? null;
  const encoded = next ? JSON.stringify(next) : null;
  if (isTauri()) {
    if (encoded) await invoke("save_auth_session", { session: encoded });
    else await invoke("delete_auth_session");
    // Never leave the production Tauri credential duplicated in web storage.
    localStorage.removeItem(BROWSER_SESSION_KEY);
  } else {
    if (encoded) localStorage.setItem(BROWSER_SESSION_KEY, encoded);
    else localStorage.removeItem(BROWSER_SESSION_KEY);
  }
}

async function authRequest(path: string, body: unknown): Promise<AuthSession> {
  const response = await fetch(`${RELAY}${path}`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
  });
  if (!response.ok) throw new Error(await errorText(response));
  const next = (await response.json()) as AuthSession;
  if (!validSession(next)) throw new Error("The relay returned an invalid session.");
  await persistSession(next);
  return next;
}

export async function login(email: string, password: string) {
  return (await authRequest("/auth/login", { email, password })).user;
}

export async function register(name: string, email: string, password: string) {
  return (await authRequest("/auth/register", { name, email, password })).user;
}

async function refreshSession(): Promise<AuthSession | null> {
  const current = await loadSession();
  if (!current?.refreshToken) return null;
  if (!pendingRefresh) {
    pendingRefresh = (async () => {
      const response = await fetch(`${RELAY}/auth/refresh`, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ refreshToken: current.refreshToken }),
      });
      if (!response.ok) {
        await persistSession(null);
        return null;
      }
      const next = (await response.json()) as AuthSession;
      if (!validSession(next)) {
        await persistSession(null);
        return null;
      }
      await persistSession(next);
      return next;
    })().finally(() => {
      pendingRefresh = null;
    });
  }
  return pendingRefresh;
}

async function freshSession(): Promise<AuthSession | null> {
  const current = await loadSession();
  if (!current) return null;
  if (current.expiresAt <= Date.now() + 30_000) return refreshSession();
  return current;
}

export async function authorizedFetch(
  input: string | URL | Request,
  init: RequestInit = {},
): Promise<Response> {
  const current = await freshSession();
  if (!current) return new Response(null, { status: 401 });
  const headers = new Headers(init.headers);
  headers.set("authorization", `Bearer ${current.token}`);
  let response = await fetch(input, { ...init, headers });
  if (response.status !== 401) return response;

  const refreshed = await refreshSession();
  if (!refreshed) return response;
  headers.set("authorization", `Bearer ${refreshed.token}`);
  response = await fetch(input, { ...init, headers });
  return response;
}

export function sessionToken() {
  return session?.token ?? null;
}

export function authenticatedUser() {
  return user;
}

export async function currentUser() {
  const response = await authorizedFetch(`${RELAY}/auth/me`);
  if (!response.ok) return null;
  const current = (await response.json()) as AuthUser;
  user = current;
  if (session) session = { ...session, user: current };
  return current;
}

export async function createPairing() {
  const response = await authorizedFetch(`${RELAY}/pairings`, { method: "POST" });
  if (!response.ok) throw new Error(await errorText(response));
  return ((await response.json()) as { code: string }).code;
}

export async function machineStatus(): Promise<MachineStatus> {
  const response = await authorizedFetch(`${RELAY}/machines`);
  if (!response.ok) return { count: 0, online: 0, machines: [] };
  return (await response.json()) as MachineStatus;
}

export async function approveDevice(userCode: string) {
  const response = await authorizedFetch(`${RELAY}/auth/device/approve`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ userCode }),
  });
  if (!response.ok) throw new Error(await errorText(response));
}

export async function revokeMachine(machineId: string) {
  const response = await authorizedFetch(`${RELAY}/machines/${encodeURIComponent(machineId)}`, {
    method: "DELETE",
  });
  if (!response.ok) throw new Error(await errorText(response));
}

export async function logout() {
  try {
    await authorizedFetch(`${RELAY}/auth/logout`, { method: "POST" });
  } finally {
    await persistSession(null);
  }
}

async function errorText(response: Response) {
  const body = await response.text();
  try {
    return (JSON.parse(body) as { error?: string }).error ?? body;
  } catch {
    return body || `Request failed (${response.status}).`;
  }
}
