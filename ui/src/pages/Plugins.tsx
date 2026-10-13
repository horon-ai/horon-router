import { Button } from "../components/ui/button";
import { useState, useSyncExternalStore, type CSSProperties } from "react";
import { useQuery } from "@tanstack/react-query";
import { api, configValue } from "../api";
import { fieldPath, readPath, type Data } from "../core";
import Editor from "../Editor";
import { Load } from "../components/Load";
import { getSnapshot as language, subscribe as subscribeLang, t } from "../lang";
import { keys, queries, queryClient } from "../queries";
import { getSnapshot, session, subscribe } from "../session";

const plain = (value: unknown) => new DOMParser().parseFromString(String(value || ""), "text/html").body.textContent || "";

const track = (on: boolean): CSSProperties => ({
  position: "relative", flex: "none", width: 44, height: 24, padding: 0, cursor: "pointer",
  borderRadius: 999, border: "1px solid var(--line)", background: on ? "var(--primary)" : "var(--panel)",
  transition: "background 0.12s",
});
const knob = (on: boolean): CSSProperties => ({
  position: "absolute", top: 2, left: on ? 22 : 2, width: 18, height: 18, borderRadius: "50%",
  background: on ? "var(--primary-foreground)" : "var(--raised)", boxShadow: "0 1px 2px rgba(0,0,0,0.2)",
  transition: "left 0.12s",
});

function Switch({ on, label, disabled, onToggle }: { on: boolean; label: string; disabled?: boolean; onToggle: () => void }) {
  return <button type="button" role="switch" aria-checked={on} aria-label={label} disabled={disabled}
    style={{ ...track(on), opacity: disabled ? 0.55 : 1 }} onClick={onToggle}><span style={knob(on)} /></button>;
}

export function Plugins() {
  useSyncExternalStore(subscribeLang, language);
  const state = useSyncExternalStore(subscribe, getSnapshot);
  const config = useQuery(queries.config());
  const plugins = useQuery(queries.plugins());
  const [browse, setBrowse] = useState(false);
  const market = useQuery({ queryKey: ["plugin-store"], queryFn: (): Promise<Data> => api("/plugins/store"), enabled: browse });
  const [edit, setEdit] = useState<{ path: string; value: unknown; title: string } | null>(null);
  const on = readPath(config.data || {}, "plugins/enabled", false) === true;
  const after = async () => { await Promise.all([queryClient.invalidateQueries({ queryKey: keys.plugins }), queryClient.invalidateQueries({ queryKey: keys.config })]); };
  function configure(id: string) {
    void session.act(async () => {
      const path = fieldPath(`plugins/configs/${id}`);
      setEdit({ path, value: await configValue(path, {}), title: id });
    });
  }
  function install(p: Data) {
    if (!confirm(t("pg.installAsk", { name: plain(p.name || p.id) }))) return;
    void session.act(async () => {
      const r = await api(`/plugins/store/${encodeURIComponent(p.id)}/install${p.source_id ? `?source=${encodeURIComponent(p.source_id)}` : ""}`, "POST", {});
      await after();
      await market.refetch();
      session.notify(r.restart_required ? t("pg.installedRestart") : t("pg.installed"));
    });
  }
  return <>
    <div className="page-head">
      <div className="page-head-copy">
        <h1>{t("pg.title")}{!!plugins.data?.length && <span>{plugins.data.length}</span>}</h1>
        <p className="page-sub">{t("pg.sub")}</p>
      </div>
      {!edit && <Button variant="quiet" size="small" disabled={state.busy} onClick={() => void session.call("PUT", fieldPath("plugins/enabled"), !on, on ? t("pg.off") : t("pg.on"), "", after)}>
        <span className={`lamp ${on ? "ok" : "off"}`} />{on ? t("pg.btnOn") : t("pg.btnOff")}</Button>}
    </div>
    {edit ? <Editor key={edit.path} {...edit} onclose={() => setEdit(null)} /> : <>
      <section className="section">
        <div className="section-head"><h2>{t("pg.installedTitle")}</h2></div>
        <Load res={plugins} what={t("what.plugins")}>{(list) => list.length
          ? <div className="stack">{list.map((p) => {
            const status = !p.registered ? { lamp: "off", word: t("pg.statusNotLoaded") }
              : p.enabled && p.effective_enabled ? { lamp: "ok", word: t("pg.statusEnabled") }
              : p.enabled ? { lamp: "warn", word: t("pg.notLoaded") }
              : { lamp: "off", word: t("common.disabled") };
            return <article className="window" key={p.id}>
              <div className="row">
                <span className="name grow"><strong className="mono">{p.metadata?.name || p.id}</strong>
                  <small>{plain(p.metadata?.description)}</small></span>
                <Switch on={!!p.enabled} disabled={state.busy}
                  label={`${p.metadata?.name || p.id} ${p.enabled ? t("common.disable") : t("common.enable")}`}
                  onToggle={() => void session.call("PUT", fieldPath(`plugins/configs/${p.id}/enabled`), !p.enabled, t("pg.savedRestart"), "", after)} />
              </div>
              <p className="row legend"><span className={`lamp ${status.lamp}`} />{status.word}</p>
              <div className="row">
                <Button size="small" disabled={state.busy} onClick={() => configure(p.id)}>{t("pg.configure")}</Button>
                <Button variant="danger" size="small" className="quiet" disabled={state.busy}
                  onClick={() => void session.call("DELETE", `/plugins/${encodeURIComponent(p.id)}`, undefined, t("pg.deleted"), t("pg.deleteAsk", { id: p.id }), after)}>{t("common.delete")}</Button>
              </div>
            </article>;
          })}</div>
          : <div className="state"><div className="row"><span className="lamp off" />{t("pg.none")}</div><p>{t("pg.noneBody")}</p></div>
        }</Load>
      </section>
      <section className="section">
        <div className="section-head"><h2>{t("pg.store")}</h2>
          <Button variant="quiet" size="small" disabled={market.isFetching || !session.can("GET", "/plugins/store")}
            onClick={() => { setBrowse(true); if (browse) void market.refetch(); }}>{t("pg.browse")}</Button></div>
        {browse && <Load res={market} what={t("what.pluginStore")}>{(m) => <>
          {(m.source_errors || []).map((e: Data, i: number) => <p className="note error" key={i}><span className="lamp bad" />{e.source_name}: {e.message}</p>)}
          {m.plugins?.length ? <div className="stack">{(m.plugins || []).map((p: Data) => <article className="window" key={`${p.source_id}/${p.id}`}>
            <div className="row">
              <span className="name grow"><strong className="mono">{plain(p.name || p.id)} <span className="legend">{p.version}</span></strong>
                <small>{plain(p.description)}</small></span>
              <Button size="small" disabled={state.busy} onClick={() => install(p)}>{p.installed ? (p.update_available ? t("pg.update") : t("pg.reinstall")) : t("pg.install")}</Button>
            </div>
          </article>)}</div> : <div className="state"><p>{t("pg.storeEmpty")}</p></div>}
        </>}</Load>}
      </section>
    </>}
  </>;
}
