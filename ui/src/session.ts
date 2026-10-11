import { api, connect, disconnect, route, ApiError } from "./api.ts";
import type { Data } from "./core.ts";
import { t } from "./lang.ts";
import type { Key } from "./i18n.ts";
import { queryClient, keys } from "./queries.ts";

export const pages = [
  ["overview", "page.overview"], ["use", "page.use"], ["credentials", "page.credentials"],
  ["providers", "page.providers"], ["keys", "page.keys"], ["models", "page.models"],
  ["payload", "page.payload"], ["quotas", "page.quotas"], ["", ""],
  ["usage", "page.usage"], ["logs", "page.logs"], ["", ""],
  ["config", "page.config"], ["plugins", "page.plugins"], ["system", "page.system"],
] as const satisfies readonly (readonly [string, Key | ""])[];

type Meta = { version: string; commit: string; built: string };
type Quota = Record<string, { at: number; provider: string; raw: Data } | { error: string }>;
type State = {
  logged: boolean;
  theme: string;
  meta: Meta;
  online: boolean;
  busy: boolean;
  dirty: boolean;
  toast: { text: string; bad: boolean } | null;
  caps: Record<string, false>;
  quota: Quota;
};
const emptyMeta = (): Meta => ({ version: "", commit: "", built: "" });
let state: State = {
  logged: false, theme: document.documentElement.dataset.theme || "dark",
  meta: emptyMeta(), online: true, busy: false, dirty: false, toast: null, caps: {}, quota: {},
};
const listeners = new Set<() => void>();
const probed = new Set<string>();
let timer: ReturnType<typeof setTimeout> | undefined;
const update = (change: Partial<State>) => {
  state = { ...state, ...change };
  listeners.forEach((listener) => listener());
};
export const subscribe = (listener: () => void) => {
  listeners.add(listener);
  return () => { listeners.delete(listener); };
};
export const getSnapshot = () => state;

export function signInHelp(e: unknown): string {
  if (!(e instanceof ApiError)) return e instanceof Error ? e.message : String(e);
  const m = e.message;
  if (e.status === 404 || /key not set/.test(m)) return t("signin.off");
  if (/remote management disabled/.test(m)) return t("signin.local");
  if (/banned/i.test(m)) return t("signin.banned", { message: m });
  if (e.status === 401) return t("signin.wrong");
  return m;
}

export const session = {
  get logged() { return state.logged; },
  get theme() { return state.theme; },
  get meta() { return state.meta; },
  get kind(): "rust" | "go" | "unknown" {
    return state.meta.version.startsWith("hyper-router") ? "rust" : state.meta.commit || state.meta.built ? "go" : "unknown";
  },
  get online() { return state.online; },
  get busy() { return state.busy; },
  get dirty() { return state.dirty; },
  set dirty(value: boolean) { update({ dirty: value }); },
  get toast() { return state.toast; },
  get caps() { return state.caps; },
  get quota() { return state.quota; },
  set quota(value: Quota) { update({ quota: value }); },
  subscribe, getSnapshot,
  async login(secret: string) {
    update({ meta: emptyMeta() });
    connect(secret, {
      unauthorized: () => {
        if (!state.logged) return;
        session.logout();
        session.notify(t("app.rejected"), true);
      },
      missing: (method, path) => update({ caps: { ...state.caps, [`${method} ${path}`]: false } }),
      meta: (h) => update({
        online: !!h,
        meta: h ? {
          version: h.get("x-cpa-version") || h.get("x-server-version") || state.meta.version,
          commit: h.get("x-cpa-commit") || state.meta.commit,
          built: h.get("x-cpa-build-date") || state.meta.built,
        } : state.meta,
      }),
    });
    try {
      queryClient.setQueryData(keys.config, await api("/config"));
    } catch (e) {
      disconnect();
      throw new Error(signInHelp(e));
    }
    update({ logged: true });
    void queryClient.prefetchQuery({ queryKey: keys.creds, queryFn: async () => (await api("/credentials")).files || [] });
  },
  logout() {
    disconnect();
    queryClient.clear();
    probed.clear();
    update({ logged: false, caps: {}, quota: {}, dirty: false });
  },
  can(method: string, path: string) {
    return state.caps[`${method} ${route(path)}`] !== false;
  },
  probe(actions: [string, string, string][]) {
    if (session.kind === "go") return;
    for (const [method, path] of actions) {
      const id = `${method} ${path}`;
      if (probed.has(id)) continue;
      probed.add(id);
      api(path, method, method === "GET" ? undefined : {}).catch(() => {});
    }
  },
  toggleTheme() {
    const theme = state.theme === "dark" ? "light" : "dark";
    document.documentElement.dataset.theme = theme;
    update({ theme });
    try { localStorage.setItem("hyperrouter-theme", theme); } catch {}
  },
  notify(text: string, bad = false) {
    update({ toast: { text, bad } });
    clearTimeout(timer);
    if (!bad) timer = setTimeout(() => update({ toast: null }), 4000);
  },
  dismiss() {
    clearTimeout(timer);
    update({ toast: null });
  },
  async act(work: () => Promise<unknown>, success = "") {
    if (state.busy) return false;
    update({ busy: true });
    try {
      await work();
      if (success) session.notify(success);
      return true;
    } catch (e) {
      session.notify(e instanceof Error ? e.message : String(e), true);
      return false;
    } finally {
      update({ busy: false });
    }
  },
  call(method: string, path: string, body: unknown, done: string, ask = "", after: () => Promise<unknown> = () => queryClient.invalidateQueries({ queryKey: keys.creds })) {
    if (ask && !confirm(ask)) return Promise.resolve(false);
    return session.act(async () => { await api(path, method, body); await after(); }, done);
  },
};
