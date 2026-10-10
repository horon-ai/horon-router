import { Input } from "../components/ui/input";
import { Button } from "../components/ui/button";
import { useEffect, useRef, useState, useSyncExternalStore } from "react";
import { useQuery } from "@tanstack/react-query";
import { api, download } from "../api";
import type { Data } from "../core";
import { Load } from "../components/Load";
import { getSnapshot as language, subscribe as subscribeLang, t } from "../lang";
import { getSnapshot, session, subscribe } from "../session";

const MAX = 1500;
export function Logs() {
  useSyncExternalStore(subscribeLang, language);
  const state = useSyncExternalStore(subscribe, getSnapshot);
  const cursor = useRef("");
  const pane = useRef<HTMLDivElement>(null);
  const lines = useRef<string[]>([]);
  const [query, setQuery] = useState("");
  const [tail, setTail] = useState(true);
  const [own, setOwn] = useState(false);
  const [visible, setVisible] = useState(!document.hidden);
  const [reqId, setReqId] = useState(() => decodeURIComponent(location.hash.match(/^#\/logs\/([^/?]+)/)?.[1] || ""));
  const [reqText, setReqText] = useState("");
  const [reqError, setReqError] = useState("");
  const logs = useQuery({
    queryKey: ["observability", "logs"],
    queryFn: async () => {
      const follow = !pane.current || pane.current.scrollHeight - pane.current.scrollTop - pane.current.clientHeight < 40;
      const r = await api(`/observability/logs?limit=300${cursor.current ? `&cursor=${encodeURIComponent(cursor.current)}` : ""}`);
      lines.current = (cursor.current && !r["cursor-reset"] ? [...lines.current, ...(r.lines || [])] : (r.lines || [])).slice(-MAX);
      cursor.current = r["next-cursor"] || "";
      if (follow) requestAnimationFrame(() => { if (pane.current) pane.current.scrollTop = pane.current.scrollHeight; });
      return lines.current;
    },
    refetchInterval: tail && visible && session.can("GET", "/observability/logs") ? 3000 : false,
  });
  const files = useQuery({ queryKey: ["observability", "errors"], queryFn: async (): Promise<Data[]> => (await api("/observability/logs/errors")).files || [] });
  useEffect(() => {
    const change = () => setVisible(!document.hidden);
    document.addEventListener("visibilitychange", change);
    return () => document.removeEventListener("visibilitychange", change);
  }, []);
  async function inspect(id: string) {
    setReqId(id);
    setReqText("");
    setReqError("");
    try { setReqText(await api(`/observability/logs/requests/${encodeURIComponent(id)}`, "GET", undefined, "text")); }
    catch (e) { setReqError(e instanceof Error ? e.message : String(e)); }
  }
  useEffect(() => {
    const change = () => {
      const id = location.hash.match(/^#\/logs\/([^/?]+)/)?.[1];
      if (id) void inspect(decodeURIComponent(id));
    };
    window.addEventListener("hashchange", change);
    change();
    return () => window.removeEventListener("hashchange", change);
  }, []);
  const shown = (logs.data || []).filter((line) => (own || !line.includes("/management/")) && line.toLowerCase().includes(query.trim().toLowerCase()));
  const parse = (line: string) => line.match(/^\[([^\]]*)\] \[([^\]]*)\] \[(\w+)\s*\] (?:\[[^\]]*\] )?(.*)$/);
  return <>
    <div className="head"><h1>{t("lg.title")}</h1>
      <Button disabled={!session.can("GET", "/observability/logs")} aria-pressed={tail} onClick={() => setTail(!tail)}>
        <span className={`lamp ${tail ? "ok live" : "off"}`} />{tail ? t("lg.following") : t("lg.paused")}</Button>
      <Button variant="danger" className="quiet" disabled={state.busy || !session.can("DELETE", "/observability/logs")}
        onClick={() => void session.call("DELETE", "/observability/logs", undefined, t("lg.cleared"), t("lg.clearAsk"), async () => {
          cursor.current = ""; lines.current = []; await logs.refetch();
        })}>{t("lg.clear")}</Button>
    </div>
    <Load res={logs} what={t("what.logs")}>{(all) => <section className="section">
      <div className="row"><label className="search grow"><svg className="i" aria-hidden="true"><use href="#i-search" /></svg><span className="sr">{t("lg.filter")}</span>
        <Input value={query} onChange={(e) => setQuery(e.target.value)} placeholder={t("lg.filter")} /></label>
        <label className="check"><input type="checkbox" checked={own} onChange={(e) => setOwn(e.target.checked)} />{t("lg.mgmt")}</label></div>
      <div className="code-window log" ref={pane} role="log" aria-label={t("lg.log")} tabIndex={0}>
        {shown.map((line, i) => { const m = parse(line); return m
          ? <div className={m[3].toLowerCase()} key={i}><span className="t">{m[1].slice(11)}</span>
              {/[^-]/.test(m[2]) ? <a className="t" href={`#/logs/${encodeURIComponent(m[2])}`}>{m[2]}</a> : <span className="t">{m[2]}</span>}
              <span className="lvl">{m[3]}</span>{m[4]}</div>
          : <div key={i}>{line}</div>; })}
        {!shown.length && <div className="muted">{query ? t("lg.noMatch") : all.length ? "" : t("lg.empty")}</div>}
      </div>
      <p className="legend">{t("lg.count", { shown: shown.length.toLocaleString(), total: all.length.toLocaleString(), max: MAX.toLocaleString() })}</p>
    </section>}</Load>
    <section className="section"><h2>{t("lg.request")}</h2>
      <form className="form" onSubmit={(e) => { e.preventDefault(); const id = reqId.trim(); if (location.hash === `#/logs/${encodeURIComponent(id)}`) void inspect(id); else location.hash = `#/logs/${encodeURIComponent(id)}`; }}>
        <label className="field">{t("lg.reqId")}<Input required value={reqId} onChange={(e) => setReqId(e.target.value)} placeholder={t("lg.reqPh")} /></label>
        <Button type="submit">{t("lg.open")}</Button>
        {reqText && <Button type="button" onClick={() => download(new Blob([reqText]), `request-${reqId}.log`)}>{t("common.download")}</Button>}
      </form>
      {reqError && <p className="note error"><span className="lamp bad" />{reqError}</p>}
      {reqText && <pre className="code-window request">{reqText}</pre>}
    </section>
    <section className="section"><h2>{t("lg.errors")}</h2><Load res={files} what={t("what.errorLogs")}>{(list) =>
      list.length ? <ul className="list">{list.map((f) => <li className="item" key={f.name}>
        <code className="grow ellipsis">{f.name}</code><span className="legend">{f.modified ? new Date(f.modified * 1000).toLocaleString(t.lang) : ""}</span>
        <Button size="small" onClick={() => void session.act(async () => download(await api(`/observability/logs/errors/${encodeURIComponent(f.name)}`, "GET", undefined, "blob"), f.name))}>{t("common.download")}</Button>
      </li>)}</ul> : <p className="muted">{t("lg.noErrors")}</p>
    }</Load></section>
  </>;
}
