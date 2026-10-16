import { Button } from "../components/ui/button";
import { useState, useSyncExternalStore } from "react";
import { useQuery } from "@tanstack/react-query";
import { api, configValue } from "../api";
import { fieldPath, type Data } from "../core";
import Editor from "../Editor";
import { Load } from "../components/Load";
import { getSnapshot as language, subscribe as subscribeLang, t } from "../lang";
import type { Key } from "../i18n";
import { queries } from "../queries";
import { getSnapshot, session, subscribe } from "../session";

const about = ["server", "management", "access", "routing", "requests", "api-keys", "oauth", "client", "multimedia", "observability"];
type Edit = { path: string; value: unknown; title: string; yaml: boolean };
const stamp = () => new Date().toLocaleTimeString(undefined, { hourCycle: "h23", hour: "2-digit", minute: "2-digit" });

export function Config() {
  useSyncExternalStore(subscribeLang, language);
  const state = useSyncExternalStore(subscribe, getSnapshot);
  const config = useQuery(queries.config());
  const yaml = useQuery({
    queryKey: ["config", "yaml"],
    queryFn: (): Promise<string> => api("/config.yaml", "GET", undefined, "text"),
    enabled: config.isSuccess,
  });
  const [format, setFormat] = useState<"json" | "yaml">("json");
  const [reread, setReread] = useState("");
  const [edit, setEdit] = useState<Edit | null>(null);
  function open(section: string) {
    void session.act(async () => {
      const path = section ? fieldPath(section) : "/config.yaml";
      setEdit({ path, value: section ? await configValue(path, {}) : await api(path, "GET", undefined, "text"), title: section || "config.yaml", yaml: !section });
      setReread(stamp());
    });
  }
  const body = format === "yaml" ? yaml.data : config.data ? JSON.stringify(config.data, null, 2) : undefined;
  return <>
    <div className="page-head">
      <div className="page-head-copy">
        <h1>{t("cf.title")}</h1>
        <p className="page-sub">{t("cf.sub")}</p>
      </div>
      <div className="seg" role="group" aria-label={t("cf.title")}>
        <button aria-pressed={format === "json"} onClick={() => setFormat("json")}>JSON</button>
        <button aria-pressed={format === "yaml"} onClick={() => setFormat("yaml")}>YAML</button>
      </div>
    </div>
    <Load res={config} what={t("what.config")}>{(data) => <>
      <section className="section window">
        <div className="section-head"><h2>{t("cf.sections")}</h2></div>
        <ul className="list">{[...new Set([...about, ...Object.keys(data)])].filter((k) => k !== "config-version").map((k) => {
          const value = (data as Data)[k];
          return <li key={k}><button className="item config-row" onClick={() => open(k)} disabled={state.busy}>
            <span className="name grow"><strong className="mono">{k}</strong>
              <small>{about.includes(k) ? t(`cf.about.${k}` as Key) : t("cf.persisted")}</small></span>
            <span className="legend">{value === undefined ? t("cf.defaults") : value && typeof value === "object" ? t("cf.nSet", { n: Object.keys(value).length }) : String(value)}</span>
            <svg className="i" aria-hidden="true"><use href="#i-chevron" /></svg>
          </button></li>;
        })}</ul>
      </section>
      {edit ? <Editor key={edit.path} {...edit} onclose={() => setEdit(null)} />
        : <section className="section window">
          <div className="section-head"><h2 className="mono">{format === "yaml" ? "config.yaml" : "config.json"}</h2>
            {reread && <span className="legend row"><svg className="i" aria-hidden="true"><use href="#i-refresh" /></svg>{t("cf.reread", { t: reread })}</span>}
            <Button size="small" disabled={state.busy} onClick={() => open("")}>{t("cf.editYaml")}</Button></div>
          <pre className="code-window request">{body ?? t("common.loading")}</pre>
        </section>}
      <p className="note">{t("cf.note")}</p>
    </>}</Load>
  </>;
}
