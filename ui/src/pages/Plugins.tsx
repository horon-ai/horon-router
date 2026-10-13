import { Button } from "../components/ui/button";
import { useState, useSyncExternalStore } from "react";
import { useQuery } from "@tanstack/react-query";
import { api, configValue } from "../api";
import { fieldPath, readPath, type Data } from "../core";
import Editor from "../Editor";
import { Load } from "../components/Load";
import { getSnapshot as language, subscribe as subscribeLang, t } from "../lang";
import { keys, queries, queryClient } from "../queries";
import { getSnapshot, session, subscribe } from "../session";

const plain = (value: unknown) => new DOMParser().parseFromString(String(value || ""), "text/html").body.textContent || "";
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
      {!edit && <>
        <Button disabled={state.busy} onClick={() => void session.call("PUT", fieldPath("plugins/enabled"), !on, on ? t("pg.off") : t("pg.on"), "", after)}>
          <span className={`lamp ${on ? "ok" : "off"}`} />{on ? t("pg.btnOn") : t("pg.btnOff")}</Button>
        <Button disabled={market.isFetching || !session.can("GET", "/plugins") || !session.can("GET", "/plugins/store")}
          onClick={() => { setBrowse(true); if (browse) void market.refetch(); }}>{t("pg.browse")}</Button>
      </>}
    </div>
    {edit ? <Editor key={edit.path} {...edit} onclose={() => setEdit(null)} /> : <>
      <Load res={plugins} what={t("what.plugins")}>{(list) => list.length
        ? <ul className="list window">{list.map((p) => <li className="item" key={p.id}>
          <span className={`lamp ${p.registered && p.effective_enabled ? "ok" : p.enabled ? "warn" : "off"}`} />
          <span className="name grow"><strong>{p.metadata?.name || p.id}</strong><small>{[p.metadata?.version, p.supports_oauth && t("pg.signin"), p.supports_quota && t("pg.quota"), !p.registered && t("pg.notLoaded")].filter(Boolean).join(" · ")}</small></span>
          <Button size="small" disabled={state.busy} onClick={() => void session.call("PUT", fieldPath(`plugins/configs/${p.id}/enabled`), !p.enabled, t("pg.savedRestart"), "", after)}>{p.enabled ? t("common.disable") : t("common.enable")}</Button>
          <Button size="small" disabled={state.busy} onClick={() => configure(p.id)}>{t("pg.configure")}</Button>
          <Button variant="danger" size="small" className="quiet" disabled={state.busy} onClick={() => void session.call("DELETE", `/plugins/${encodeURIComponent(p.id)}`, undefined, t("pg.deleted"), t("pg.deleteAsk", { id: p.id }), after)}>{t("common.delete")}</Button>
        </li>)}</ul>
        : <div className="state"><div className="row"><span className="lamp off" />{t("pg.none")}</div><p>{t("pg.noneBody")}</p></div>
      }</Load>
      {browse && <section className="section window"><h2>{t("pg.store")}</h2>
        <Load res={market} what={t("what.pluginStore")}>{(m) => <>
          {(m.source_errors || []).map((e: Data, i: number) => <p className="note error" key={i}><span className="lamp bad" />{e.source_name}: {e.message}</p>)}
          <ul className="list">{(m.plugins || []).map((p: Data) => <li className="item" key={`${p.source_id}/${p.id}`}>
            <span className="name grow"><strong>{plain(p.name || p.id)} <span className="legend">{p.version}</span></strong><small>{plain(p.description)}</small></span>
            <Button size="small" disabled={state.busy} onClick={() => install(p)}>{p.installed ? (p.update_available ? t("pg.update") : t("pg.reinstall")) : t("pg.install")}</Button>
          </li>)}{!m.plugins?.length && <li className="state"><p>{t("pg.storeEmpty")}</p></li>}</ul>
        </>}</Load>
      </section>}
    </>}
  </>;
}
