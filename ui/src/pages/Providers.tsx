import { Input } from "../components/ui/input";
import { Button } from "../components/ui/button";
import { useState, useSyncExternalStore } from "react";
import { useQuery } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { api, missing, text } from "../api";
import { label, mask, readPath, sum, type Buckets, type Data } from "../core";
import { Grille } from "../components/Grille";
import { Load } from "../components/Load";
import { getSnapshot as language, subscribe as subscribeLang, t } from "../lang";
import { queries, replace } from "../queries";
import { getSnapshot, session, subscribe } from "../session";

const families = ["claude", "codex", "gemini", "vertex", "openai-compatibility", "interactions", "xai", "meta"];

export function Providers() {
  useSyncExternalStore(subscribeLang, language);
  const state = useSyncExternalStore(subscribe, getSnapshot);
  const config = useQuery(queries.config());
  const usage = useQuery({
    queryKey: ["providers", "api-key-usage"],
    queryFn: (): Promise<Data> => api("/observability/usage/api-keys"),
    refetchInterval: 15_000,
  });
  const [family, setFamily] = useState("claude");
  const [reveal, setReveal] = useState(false);
  const [form, setForm] = useState({ name: "", url: "", key: "" });
  const groups = readPath(config.data || {}, `api-keys/${family}`, []) as Data[];
  const compat = family === "openai-compatibility";
  function traffic(group: Data, key: Data): Buckets | null {
    const byKey = usage.data?.[compat ? String(group.name || "").toLowerCase() : family];
    const hit = byKey?.[`${group["base-url"] || ""}|${key["api-key"]}`];
    if (!hit) return null;
    const rows: Data[] = hit.recent_requests || [];
    return { total: rows.map((row) => (row.success || 0) + (row.failed || 0)),
      failed: rows.map((row) => row.failed || 0) };
  }
  const max = Math.max(1, ...groups.flatMap((group) =>
    (group.keys || []).flatMap((key: Data) => traffic(group, key)?.total || [])));
  const write = (next: Data[], done: string) =>
    session.act(() => replace(`api-keys/${family}`, groups, next), done);
  function add(event: React.SubmitEvent) {
    event.preventDefault();
    const group: Data = { name: form.name.trim(), keys: [{ "api-key": form.key.trim() }] };
    if (form.url.trim()) group["base-url"] = form.url.trim();
    void write([...groups, group], t("pv.groupAdded")).then((ok) =>
      ok && setForm({ name: "", url: "", key: "" }));
  }
  return <>
    <div className="head">
      <h1>{t("pv.title")}</h1>
      {!!groups.length && <Button variant="quiet" aria-pressed={reveal}
        onClick={() => setReveal(!reveal)}>{t(reveal ? "common.hideKeys" : "common.showKeys")}</Button>}
      <Link className="key" to="/config" activeProps={{}}><svg className="i" aria-hidden="true"><use href="#i-edit" /></svg>{t("common.edit")}</Link>
    </div>
    <div className="seg" role="group" aria-label={t("pv.provider")}>
      {families.map((f) => <button key={f} aria-pressed={family === f} onClick={() => setFamily(f)}>
        {label(f, t)}<b>{readPath(config.data || {}, `api-keys/${f}`, []).length || ""}</b>
      </button>)}
    </div>
    <Load res={config} what={t("what.providers")}>{() => <>
      {groups.length ? <ul className="list groups">{groups.map((group, i) =>
        <li className="stack" key={i}>
          <div className="section-head">
            <h2>{group.name || t("pv.group", { n: i + 1 })}</h2>
            {group.disabled && <span className="row legend"><span className="lamp off" />{t("common.disabled")}</span>}
            {compat && <Button size="small" disabled={state.busy}
              onClick={() => void write(groups.map((item, n) => n === i ? { ...item, disabled: !item.disabled } : item),
                t(group.disabled ? "pv.groupEnabled" : "pv.groupDisabled"))}>
              {t(group.disabled ? "common.enable" : "common.disable")}</Button>}
            <Button variant="danger" size="small" className="quiet" disabled={state.busy}
              onClick={() => window.confirm(t("pv.deleteAsk", { name: group.name || i + 1 })) &&
                void write(groups.filter((_, n) => n !== i), t("pv.groupDeleted"))}>{t("common.delete")}</Button>
          </div>
          <p className="legend mono">{group["base-url"] || t("pv.default")}</p>
          {(group.keys || []).map((key: Data, n: number) => {
            const tr = traffic(group, key);
            return <div className="item key-row" key={n}>
              <code className="grow ellipsis">{reveal ? key["api-key"] : mask(String(key["api-key"] || ""))}</code>
              <span className="legend">{t("pv.weight", { n: key.weight ?? 1 })}</span>
              {tr && <><Grille data={tr} max={max} /><span className="num count">{sum(tr.total).toLocaleString()}</span></>}
            </div>;
          })}
          {!!group.models?.length && <div className="chips">{group.models.map((model: Data, n: number) =>
            <span className="chip" key={n}>{model.name}{model.alias && model.alias !== model.name ? ` → ${model.alias}` : ""}</span>)}</div>}
        </li>)}</ul> : <div className="state">
        <div className="row"><span className="lamp off" />{t("pv.none", { provider: label(family, t) })}</div>
        <p>{t("pv.noneBody")}</p>
      </div>}
      {usage.error && <p className="note"><span className={`lamp ${missing(usage.error) ? "off" : "bad"}`} />
        {missing(usage.error) ? t("pv.noTraffic") : text(usage.error)}</p>}
      <form className="form" onSubmit={add}>
        <label className="field">{t("pv.groupName")}<Input required value={form.name}
          onChange={(e) => setForm({ ...form, name: e.target.value })} /></label>
        <label className="field">{t("pv.baseUrl")}<Input type="url" required={compat} value={form.url}
          onChange={(e) => setForm({ ...form, url: e.target.value })}
          placeholder={compat ? "https://…/v1" : t("pv.defaultPh")} /></label>
        <label className="field">{t("pv.apiKey")}<Input type="password" autoComplete="off" required value={form.key}
          onChange={(e) => setForm({ ...form, key: e.target.value })} /></label>
        <Button type="submit" variant="primary" disabled={state.busy}>
          <svg className="i" aria-hidden="true"><use href="#i-plus" /></svg>{t("pv.addGroup")}</Button>
      </form>
    </>}</Load>
  </>;
}
