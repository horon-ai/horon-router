import { Button } from "../components/ui/button";
import { useSyncExternalStore } from "react";
import { useQuery } from "@tanstack/react-query";
import { api, ApiError, text } from "../api";
import { readPath } from "../core";
import { getSnapshot as language, subscribe as subscribeLang, t } from "../lang";
import { queries } from "../queries";
import { getSnapshot, session, subscribe } from "../session";

const clean = (v: string) => v.replace(/^horon-router[/-]/, "").replace(/^v/, "");
export function System() {
  useSyncExternalStore(subscribeLang, language);
  const state = useSyncExternalStore(subscribe, getSnapshot);
  const config = useQuery(queries.config());
  const latest = useQuery({ queryKey: ["server", "latest-version"], queryFn: async (): Promise<string> => String((await api("/server/latest-version"))["latest-version"] || ""), enabled: false });
  const c = config.data || {};
  const keys = (readPath(c, "access/api-keys", []) as string[]).length;
  // A failed config read keeps its last good data in the cache; say so instead of
  // showing that stale data as current.
  const failed = config.isError;
  const disabled = latest.error instanceof ApiError && latest.error.code === "update_check_disabled";
  const newer = !disabled && latest.data !== undefined && clean(latest.data) !== clean(state.meta.version);
  // ponytail: the canvas's fifth detail row is the config file path, which the management
  // API does not report; its row shows the config layout instead. Widen when a path is served.
  const details: [string, unknown, boolean?][] = [
    [t("sy.build"), session.kind === "go" ? `CLIProxyAPI ${state.meta.version} (Go)` : session.kind === "rust" ? state.meta.version.replace("/", " ") : state.meta.version || t("sy.notReported")],
    [t("sy.commit"), state.meta.commit !== "none" && state.meta.commit, true],
    [t("sy.built"), !isNaN(Date.parse(state.meta.built)) && new Date(state.meta.built).toLocaleString(t.lang)],
    [t("sy.authDir"), readPath(c, "oauth/auth-dir", t("sy.default"))],
    [t("sy.layout"), c["config-version"] ? `v${c["config-version"]}` : t("sy.legacy")],
  ];
  const build = disabled ? t("sy.disabled")
    : latest.data !== undefined ? (newer ? t("sy.available", { v: clean(latest.data) }) : t("sy.upToDate").replace(/^ · /, ""))
      : latest.isFetching ? t("common.checking") : t("sy.nothing");
  return <>
    <div className="page-head">
      <div className="page-head-copy">
        <h1>{t("sy.title")}</h1>
        <p className="page-sub">{t("sy.sub")}</p>
      </div>
      <Button disabled={latest.isFetching || disabled} onClick={() => void latest.refetch()}>
        <svg className="i" aria-hidden="true"><use href="#i-refresh" /></svg>{t("sy.checkNew")}</Button>
    </div>
    {failed && <p className="note error" role="alert"><span className="lamp bad" />{text(config.error)}</p>}
    <section style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(220px, 1fr))", gap: 16 }} aria-label={t("sy.title")}>
      <div className="window grow">
        <div className="row"><span className={`lamp ${state.online ? "ok" : "bad"}`} /><h2>{t("sy.running")}</h2></div>
        <p className="muted">{t("sy.serving")}</p>
      </div>
      <div className="window grow">
        <div className="row"><span className={`lamp ${failed ? "bad" : keys ? "ok" : "off"}`} /><h2>{failed ? t("common.notReported") : `${keys} ${t("sy.clientKeys")}`}</h2></div>
      </div>
      <div className="window grow">
        <div className="row"><span className={`lamp ${newer ? "warn" : "ok"}`} /><h2>{t("sy.newer")}</h2></div>
        <p className="muted">{build}</p>
      </div>
    </section>
    <section className="section window">
      <h2>{t("sy.details")}</h2>
      <dl className="facts">{details.filter((f) => f[1]).map(([key, value, mono]) =>
        <div key={key}><dt>{key}</dt><dd className={mono ? "mono" : undefined}>{String(value)}</dd></div>)}</dl>
    </section>
    <p className="note">{t("sy.onDemand")}</p>
  </>;
}
