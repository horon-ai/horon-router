import type { Data } from "./core.ts";
import { t } from "./lang.ts";

// The management key lives only in this module's memory. It is never written to storage.
// Requests go only to the server that served this page: the API sits next to it
// (/management.html -> /v8/management, also under a reverse-proxy path prefix).
export const base = new URL(".", location.href).href.replace(/\/$/, "");
let key = "";
let revision = 0;
let hooks: {
  unauthorized(): void;
  missing(method: string, path: string): void;
  meta(h: Headers | null): void;
} = { unauthorized() {}, missing() {}, meta() {} };

export class ApiError extends Error {
  status: number;
  code: string;
  method: string;
  path: string;
  constructor(
    message: string,
    status: number,
    code: string,
    method: string,
    path: string,
  ) {
    super(message);
    this.status = status;
    this.code = code;
    this.method = method;
    this.path = path;
  }
  /** The server does not implement this route or method (Rust 501/405, Go's empty 404). */
  get missing() {
    return this.status === 501 || this.status === 405 || (this.status === 404 && !this.code);
  }
}

export function connect(secret: string, h: typeof hooks) {
  key = secret;
  hooks = h;
  revision++;
}
export function disconnect() {
  key = "";
  revision++;
}
export const endpoint = () => `${base}/v8/management`;
export const route = (path: string) => path.split("?")[0];

async function send(path: string, method: string, body?: unknown) {
  const at = revision;
  // No key is a valid state: a server with no password accepts the request, and one that
  // needs a key answers 401. Omitting the header lets the server tell the two apart.
  const headers: Record<string, string> = {};
  if (key) headers.Authorization = `Bearer ${key}`;
  const yaml = path === "/config.yaml";
  if (body !== undefined && !(body instanceof FormData))
    headers["Content-Type"] = yaml ? "application/yaml" : "application/json";
  let response: Response;
  try {
    response = await fetch(endpoint() + path, {
      method,
      headers,
      body: body === undefined || body instanceof FormData || yaml ? (body as BodyInit) : JSON.stringify(body),
      signal: AbortSignal.timeout(path.includes("/refresh") ? 300_000 : 30_000),
      cache: "no-store",
      redirect: "error",
    });
  } catch (e) {
    if (at === revision) hooks.meta(null);
    throw new Error(
      at !== revision
        ? t("api.changed")
        : e instanceof Error && e.name === "TimeoutError"
          ? t("api.timeout")
          : t("api.unreachable"),
    );
  }
  if (at !== revision) throw new Error(t("api.changed"));
  hooks.meta(response.headers);
  return response;
}

export async function api(
  path: string,
  method = "GET",
  body?: unknown,
  format: "json" | "text" | "blob" = "json",
): Promise<any> {
  const at = revision;
  const response = await send(path, method, body);
  const ok = response.ok;
  const value = !ok
    ? await response.json().catch(() => ({}))
    : format === "blob"
      ? await response.blob()
      : format === "text"
        ? await response.text()
        : response.status === 204
          ? {}
          : await response.json();
  // A body that finishes after sign-out or a reconnect belongs to the old session.
  if (at !== revision) throw new Error(t("api.changed"));
  if (ok) return value;
  if (response.status === 401) hooks.unauthorized();
  const error = new ApiError(
    String(value.message || value.error || `HTTP ${response.status}`),
    response.status,
    String(value.code || value.error || ""),
    method,
    route(path),
  );
  if (error.missing) {
    error.message = t("api.missing", { method, path: route(path), status: response.status });
    hooks.missing(method, route(path));
  }
  throw error;
}
/** True when the server lacks the route; other errors are real failures. */
export const missing = (e: unknown) => e instanceof ApiError && e.missing;
export const text = (e: unknown) => (e instanceof Error ? e.message : String(e));

/** Asks this proxy for its model list with a client key, as a tool would. No upstream call. */
export async function clientModels(clientKey: string): Promise<string[]> {
  const r = await fetch(`${base}/v1/models`, { headers: { Authorization: `Bearer ${clientKey}` }, cache: "no-store" });
  if (r.status === 401) throw new Error(t("api.keyRejected"));
  if (!r.ok) throw new Error(t("api.httpStatus", { status: r.status }));
  return ((await r.json()).data || []).map((m: Data) => String(m.id));
}

export async function configValue(path: string, fallback: unknown) {
  try {
    return await api(path);
  } catch (e) {
    if (e instanceof ApiError && e.status === 404 && e.code === "not_found") return fallback;
    throw e;
  }
}

export function download(blob: Blob, filename: string) {
  const url = URL.createObjectURL(blob),
    a = document.createElement("a");
  a.href = url;
  a.download = filename;
  a.click();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}
