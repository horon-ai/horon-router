import { Button } from "../components/ui/button";
import { useState, useSyncExternalStore } from "react";
import { useQuery } from "@tanstack/react-query";
import { api, configValue } from "../api";
import { fieldPath, readPath, strategies, strategy } from "../core";
import Editor from "../Editor";
import { Load } from "../components/Load";
import { getSnapshot as language, subscribe as subscribeLang, t } from "../lang";
import type { Key } from "../i18n";
import { queries, replace } from "../queries";
import { getSnapshot, session, subscribe } from "../session";

const about = ["server", "management", "access", "routing", "requests", "api-keys", "oauth", "client", "multimedia", "observability", "plugins"];
type Edit = { path: string; value: unknown; title: string; yaml: boolean };
export function Config() {
  useSyncExternalStore(subscribeLang, language);
  const state = useSyncExternalStore(subscribe, getSnapshot);
  const config = useQuery(queries.config());
  const [edit, setEdit] = useState<Edit | null>(null);
  const saved = readPath(config.data || {}, "routing/strategy", "") as string;
  const current = strategy(saved);
  function open(section: string) {
    void session.act(async () => {
      const path = section ? fieldPath(section) : "/config.yaml";
      setEdit({ path, value: section ? await configValue(path, {}) : await api(path, "GET", undefined, "text"), title: section || "config.yaml", yaml: !section });
    });
  }
  return <>
    <div className="page-head">
      <div className="page-head-copy">
        <h1>{t("cf.title")}</h1>
        <p className="page-sub">{t("cf.sub")}</p>
      </div>
      {!edit && <Button disabled={state.busy} onClick={() => open("")}><svg className="i" aria-hidden="true"><use href="#i-edit" /></svg>{t("cf.editYaml")}</Button>}
    </div>
    {edit ? <Editor key={edit.path} {...edit} onclose={() => setEdit(null)} />
      : <Load res={config} what={t("what.config")}>{(data) => <>
        <section className="section window"><h2>{t("cf.strategy")}</h2>
          <div className="seg" role="group" aria-label={t("cf.strategy")}>
            {strategies.filter((s) => !s.rust || session.kind === "rust").map((s) =>
              <button key={s.value} aria-pressed={current.value === s.value} disabled={state.busy}
                onClick={() => { if (current.value !== s.value) void session.act(() => replace("routing/strategy", saved, s.value, ""), t("cf.strategySet", { name: t(strategy(s.value).name).toLowerCase() })); }}>
                {t(s.name)}</button>)}
          </div><p className="muted">{t(current.help)}</p>
        </section>
        <ul className="list window">{[...new Set([...about, ...Object.keys(data)])].filter((k) => k !== "config-version").map((k) => {
          const value = data[k];
          return <li key={k}><button className="item config-row" onClick={() => open(k)} disabled={state.busy}>
            <span className="name grow"><strong className="mono">{k}</strong><small>{about.includes(k) ? t(`cf.about.${k}` as Key) : t("cf.persisted")}</small></span>
            <span className="legend">{value === undefined ? t("cf.defaults") : value && typeof value === "object" ? t("cf.nSet", { n: Object.keys(value).length }) : String(value)}</span>
            <svg className="i" aria-hidden="true"><use href="#i-chevron" /></svg>
          </button></li>;
        })}</ul><p className="note">{t("cf.note")}</p>
      </>}</Load>}
  </>;
}
