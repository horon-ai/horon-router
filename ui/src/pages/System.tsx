import { Button } from "../components/ui/button";
import { useSyncExternalStore } from "react";
import { useQuery } from "@tanstack/react-query";
import { api, endpoint, ApiError } from "../api";
import { readPath } from "../core";
import { Load } from "../components/Load";
import { Rich } from "../components/Rich";
import { getSnapshot as language, subscribe as subscribeLang, t } from "../lang";
import { queries } from "../queries";
import { getSnapshot, session, subscribe } from "../session";

const clean = (v: string) => v.replace(/^hyper-router[/-]/, "").replace(/^v/, "");
export function System() {
  useSyncExternalStore(subscribeLang, language);
  const state = useSyncExternalStore(subscribe, getSnapshot);
  const config = useQuery(queries.config());
  const latest = useQuery({ queryKey: ["server", "latest-version"], queryFn: async (): Promise<string> => String((await api("/server/latest-version"))["latest-version"] || ""), enabled: false });
  const c = config.data || {};
  const facts: [string, unknown, boolean?][] = [
    [t("sy.api"), new URL(endpoint()).pathname, true],
    [t("sy.server"), session.kind === "go" ? `CLIProxyAPI ${state.meta.version} (Go)` : session.kind === "rust" ? state.meta.version.replace("/", " ") : state.meta.version || t("sy.notReported")],
    [t("sy.commit"), state.meta.commit !== "none" && state.meta.commit, true],
    [t("sy.built"), !isNaN(Date.parse(state.meta.built)) && new Date(state.meta.built).toLocaleString(t.lang)],
    [t("sy.layout"), c["config-version"] ? `v${c["config-version"]}` : t("sy.legacy")],
    [t("sy.strategy"), readPath(c, "routing/strategy", t("sy.default"))],
    [t("sy.authDir"), readPath(c, "oauth/auth-dir", t("sy.default"))],
    [t("sy.remote"), readPath(c, "management/allow-remote", false) ? t("sy.allowed") : t("sy.localOnly")],
    [t("sy.dashboard"), session.kind === "rust" ? t("sy.builtIn") : readPath(c, "management/disable-auto-update-panel", false) ? t("sy.localFile") : t("sy.autoUpdate")],
  ];
  const disabled = latest.error instanceof ApiError && latest.error.code === "update_check_disabled";
  return <>
    <div className="head"><h1>{t("sy.title")}</h1></div>
    <section className="section"><dl className="facts">{facts.filter((f) => f[1]).map(([key, value, mono]) =>
      <div key={key}><dt>{key}</dt><dd className={mono ? "mono" : undefined}>{String(value)}</dd></div>)}</dl>
      <p className="note">{t("sy.noProc")}</p>
    </section>
    <section className="section"><div className="section-head"><h2>{t("sy.updates")}</h2>
      <Button disabled={latest.isFetching || disabled} onClick={() => void latest.refetch()}>{t("sy.checkNew")}</Button></div>
      {disabled ? <p className="note" role="status"><Rich text={t("sy.disabled")} /></p>
        : latest.data !== undefined || latest.error || latest.isFetching
          ? <Load res={latest} what={t("what.release")}>{(v) => <p className="row">
            <span className={`lamp ${clean(v) === clean(state.meta.version) ? "ok" : "warn"}`} />
            {t("sy.latest", { name: session.kind === "rust" ? "HyperRouter" : "CLIProxyAPI", v: v || t("sy.unknown"), upToDate: clean(v) === clean(state.meta.version) ? t("sy.upToDate") : "" })}
          </p>}</Load>
          : <p className="muted">{t("sy.nothing")}{session.kind === "rust" ? t("sy.rustNote") : t("sy.goNote")}</p>}
    </section>
  </>;
}
