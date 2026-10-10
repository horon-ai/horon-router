import { Input } from "../components/ui/input";
import { Button } from "../components/ui/button";
import { useState, useSyncExternalStore } from "react";
import { useQuery } from "@tanstack/react-query";
import { api } from "../api";
import { label, readPath, type Data } from "../core";
import { Load } from "../components/Load";
import { getSnapshot as language, subscribe as subscribeLang, t } from "../lang";
import { queries, replace } from "../queries";
import { getSnapshot, session, subscribe } from "../session";

const channels = ["claude", "codex", "antigravity", "kimi", "xai", "meta", "command-code", "nous", "github-copilot", "vertex", "aistudio"];
const count = (n: number) => n >= 1000 ? `${Math.round(n / 1000)}k` : String(n || "—");

export function Models() {
  useSyncExternalStore(subscribeLang, language);
  const state = useSyncExternalStore(subscribe, getSnapshot);
  const config = useQuery(queries.config());
  const [channel, setChannel] = useState("claude");
  const [alias, setAlias] = useState({ name: "", alias: "", fork: false });
  const [pattern, setPattern] = useState("");
  const aliases = readPath(config.data || {}, `oauth/model-alias/${channel}`, []) as Data[];
  const excluded = readPath(config.data || {}, `oauth/excluded-models/${channel}`, []) as string[];
  const catalog = useQuery({
    queryKey: ["routing", "model-definitions", channel],
    queryFn: async (): Promise<Data[]> =>
      (await api(`/routing/model-definitions/${encodeURIComponent(channel)}`)).models || [],
  });
  function addAlias(event: React.SubmitEvent) {
    event.preventDefault();
    const next = { name: alias.name.trim(), alias: alias.alias.trim(), ...(alias.fork ? { fork: true } : {}) };
    if (aliases.some((item) => item.alias === next.alias)) {
      session.notify(t("md.aliasExists"), true);
      return;
    }
    void session.act(() => replace(`oauth/model-alias/${channel}`, aliases, [...aliases, next]), t("md.aliasAdded"))
      .then((ok) => ok && setAlias({ name: "", alias: "", fork: false }));
  }
  function exclude(event: React.SubmitEvent) {
    event.preventDefault();
    void session.act(() => replace(`oauth/excluded-models/${channel}`, excluded,
      [...new Set([...excluded, pattern.trim()])]), t("md.excludedDone"))
      .then((ok) => ok && setPattern(""));
  }
  return <>
    <div className="head"><h1>{t("md.title")}</h1></div>
    <div className="seg" role="group" aria-label={t("md.channel")}>
      {channels.map((item) => <button key={item} aria-pressed={channel === item}
        onClick={() => setChannel(item)}>{label(item, t)}</button>)}
    </div>
    <Load res={config} what={t("what.modelSettings")}>{() => <>
      <section className="section">
        <div className="section-head"><h2>{t("md.aliases")}</h2><span className="legend">{t("md.aliasHelp")}</span></div>
        {aliases.length ? <ul className="list">{aliases.map((item, i) =>
          <li className="item" key={i}>
            <code>{item.name}</code><svg className="i" width="14" height="14" aria-hidden="true"><use href="#i-chevron" /></svg>
            <code className="grow">{item.alias}</code>
            {item.fork && <span className="legend">{t("md.keepsOriginal")}</span>}
            <Button variant="danger" size="small" className="quiet" disabled={state.busy}
              onClick={() => void session.act(() => replace(`oauth/model-alias/${channel}`,
                aliases, aliases.filter((_, n) => n !== i)), t("md.aliasRemoved"))}>{t("common.remove")}</Button>
          </li>)}</ul> : <p className="muted">{t("md.noAliases")}</p>}
        <form className="form" onSubmit={addAlias}>
          <label className="field">{t("md.upstream")}<Input list="defs" required value={alias.name}
            onChange={(e) => setAlias({ ...alias, name: e.target.value })} /></label>
          <label className="field">{t("md.alias")}<Input required value={alias.alias}
            onChange={(e) => setAlias({ ...alias, alias: e.target.value })} /></label>
          <label className="check"><input type="checkbox" checked={alias.fork}
            onChange={(e) => setAlias({ ...alias, fork: e.target.checked })} />{t("md.keepOriginal")}</label>
          <Button type="submit" disabled={state.busy}>
            <svg className="i" aria-hidden="true"><use href="#i-plus" /></svg>{t("md.addAlias")}</Button>
        </form>
      </section>
      <section className="section">
        <div className="section-head"><h2>{t("md.excluded")}</h2><span className="legend">{t("md.excludedHelp")}</span></div>
        <div className="chips">{excluded.length ? excluded.map((item, i) =>
          <span className="chip" key={i}>{item}<button aria-label={t("md.allow", { name: item })}
            disabled={state.busy} onClick={() => void session.act(() =>
              replace(`oauth/excluded-models/${channel}`, excluded, excluded.filter((_, n) => n !== i)))}>
            <svg className="i" width="12" height="12" aria-hidden="true"><use href="#i-close" /></svg>
          </button></span>) : <span className="muted">{t("md.all")}</span>}</div>
        <form className="form" onSubmit={exclude}>
          <label className="field">{t("md.pattern")}<Input required value={pattern} placeholder="*-preview"
            onChange={(e) => setPattern(e.target.value)} /></label>
          <Button type="submit" disabled={state.busy}>{t("md.exclude")}</Button>
        </form>
      </section>
    </>}</Load>
    <section className="section">
      <div className="section-head"><h2>{t("md.catalog")}</h2>
        {catalog.data && <span className="legend">{t("md.count", { n: catalog.data.length })}</span>}</div>
      <Load res={catalog} what={t("what.catalog")}>{(list) => list.length
        ? <><ul className="list">
          <li className="item catalog legend" aria-hidden="true"><span>{t("md.colModel")}</span><span>{t("md.colName")}</span>
            <span className="count">{t("md.colContext")}</span><span className="count">{t("md.colOutput")}</span></li>
          {list.map((model) => <li className="item catalog" key={model.id}>
            <code className="ellipsis">{model.id}</code><span className="ellipsis">{model.display_name || ""}</span>
            <span className="num count">{count(model.context_length)}</span>
            <span className="num count">{count(model.max_completion_tokens)}</span>
          </li>)}</ul>
          <datalist id="defs">{list.map((model) => <option key={model.id} value={model.id} />)}</datalist></>
        : <p className="muted">{t("md.noDefs", { provider: label(channel, t) })}</p>}</Load>
    </section>
  </>;
}
