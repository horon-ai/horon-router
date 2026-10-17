import { QueryClient } from "@tanstack/react-query";
import { api, configValue } from "./api.ts";
import { equal, fieldPath, type Data } from "./core.ts";
import { t } from "./lang.ts";

export const queryClient = new QueryClient({
  defaultOptions: { queries: { refetchIntervalInBackground: false, retry: false } },
});
export const keys = {
  config: ["config"], creds: ["creds"], usage: ["usage"],
  logs: ["logs"], system: ["system"], tailscale: ["tailscale"],
} as const;
export const queries = {
  config: () => ({ queryKey: keys.config, queryFn: (): Promise<Data> => api("/config") }),
  creds: () => ({ queryKey: keys.creds, queryFn: async (): Promise<Data[]> => (await api("/credentials")).files || [] }),
  usage: () => ({ queryKey: keys.usage, queryFn: (): Promise<Data> => api("/usage") }),
  logs: () => ({ queryKey: keys.logs, queryFn: (): Promise<Data> => api("/logs") }),
  system: () => ({ queryKey: keys.system, queryFn: (): Promise<Data> => api("/system") }),
  // horon-router only; a Go server answers the route with its empty 404.
  tailscale: () => ({ queryKey: keys.tailscale, queryFn: (): Promise<Data> => api("/server/tailscale") }),
};
// Pages opt in to polling: credentials every 10s, logs every 3s.
export async function replace(path: string, before: unknown, next: unknown, unset: unknown = Array.isArray(before) ? [] : {}) {
  const url = fieldPath(path);
  const latest = await configValue(url, unset);
  if (!equal(latest, before)) {
    await queryClient.invalidateQueries({ queryKey: keys.config });
    throw new Error(t("store.changed"));
  }
  await api(url, "PUT", next);
  await queryClient.invalidateQueries({ queryKey: keys.config });
}
