import { Button } from "../components/ui/button";
import { useEffect, useState, useSyncExternalStore } from "react";
import { useQuery } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { api, missing, text } from "../api";
import { buckets, sumBuckets, sum, label, provider, cleanEvent, usageStats, type Buckets, type Data, type Event } from "../core";
import { Grille } from "../components/Grille";
import { t, subscribe as subscribeLang, getSnapshot as language } from "../lang";
import { queries } from "../queries";
import { session } from "../session";

const apiKeysQuery = { queryKey: ["observability", "usage", "api-keys"], queryFn: (): Promise<Data> => api("/observability/usage/api-keys") };
const toBuckets = (rows: Data[]): Buckets => ({
  total: rows.map((b) => (b.success || 0) + (b.failed || 0)),
  failed: rows.map((b) => b.failed || 0),
});
const fmt = (n: number | null, unit = "") => n === null ? "—" : `${n.toLocaleString()}${unit}`;

export function Usage() {
  useSyncExternalStore(subscribeLang, language);
  useSyncExternalStore(session.subscribe, session.getSnapshot);
  const creds = useQuery({ ...queries.creds(), refetchInterval: 15_000 });
  const keys = useQuery({ ...apiKeysQuery, refetchInterval: 15_000 });
  const [live, setLive] = useState(false);
  const [events, setEvents] = useState<Event[]>([]);
  const [problem, setProblem] = useState("");
  const rows = new Map<string, { name: string; via: string; b: Buckets | null }>();
  for (const a of creds.data || []) {
    const p = provider(a);
    const prev = rows.get(`a:${p}`);
    rows.set(`a:${p}`, { name: label(p, t), via: t("us.accounts"), b: sumBuckets([prev?.b || null, buckets(a)]) });
  }
  for (const [p, entries] of Object.entries<Data>(keys.data || {}))
    rows.set(`k:${p}`, {
      name: label(p, t), via: t("us.apiKeys"),
      b: sumBuckets(Object.values<Data>(entries).map((e) => toBuckets(e.recent_requests || []))),
    });
  const sorted = [...rows.values()].sort((x, y) => sum(y.b?.total || []) - sum(x.b?.total || []));
  const max = Math.max(1, ...sorted.flatMap((r) => r.b?.total || []));
  const stats = usageStats(events);
  const time = (at: number) => new Date(at).toLocaleTimeString(t.lang, {
    hourCycle: "h23", hour: "2-digit", minute: "2-digit", second: "2-digit",
  });

  useEffect(() => {
    if (!live) return;
    let active = true;
    async function collect() {
      try {
        const got = await api("/observability/usage/queue?count=500");
        if (!active) return;
        const cutoff = Date.now() - 60 * 60_000;
        if (Array.isArray(got) && got.length)
          setEvents((before) => [...before, ...got.filter((r) => r && typeof r === "object").map(cleanEvent)]
            .filter((e) => e.at > cutoff).slice(-2000));
        setProblem("");
      } catch (error) {
        if (!active) return;
        setProblem(error instanceof Error ? error.message : String(error));
        setLive(false);
      }
    }
    void collect();
    const timer = setInterval(() => void collect(), 3000);
    return () => { active = false; clearInterval(timer); };
  }, [live]);

  return <>
    <div className="page-head">
      <div className="page-head-copy">
        <h1>{t("us.title")}</h1>
        <p className="page-sub">{t("us.sub")}</p>
      </div>
      <Button disabled={!session.can("GET", "/observability/usage/queue")}
        aria-pressed={live} onClick={() => {
          if (live || confirm(t("us.ask"))) setLive(!live);
        }}><span className={`lamp ${live ? "ok live" : "off"}`} />{live ? t("us.stop") : t("us.start")}</Button>
    </div>
    <section className="section window">
      <div className="section-head"><h2>{t("us.last200")}</h2><span className="legend">{t("us.reported")}</span></div>
      {sorted.length ? <ul className="list">{sorted.map((r) => <li className="item usage-row" key={`${r.name}:${r.via}`}>
        <span className="name grow"><strong>{r.name}</strong><small>{r.via}</small></span>
        {r.b ? <><Grille data={r.b} max={max} /><span className="num count">{sum(r.b.total).toLocaleString()}</span>
          <span className="num count legend">{sum(r.b.failed) ? t("us.failedN", { n: sum(r.b.failed) }) : ""}</span></>
          : <span className="legend">{t("common.notReported")}</span>}
      </li>)}</ul> : <p className="muted">{creds.error ? text(creds.error) : creds.data ? t("us.none") : t("common.loading")}</p>}
      {keys.error && <p className="note"><span className={`lamp ${missing(keys.error) ? "off" : "bad"}`} />
        {missing(keys.error) ? t("us.keysNA") : text(keys.error)}</p>}
    </section>
    <section className="section">
      <div className="section-head">
        <h2>{t("us.live")}</h2>
      </div>
      {problem && <p className="note error" role="alert"><span className="lamp bad" />{problem}</p>}
      {!events.length ? <p className="muted">{live ? t("us.listening") : t("us.hint")}</p> : <>
        <div className="window display stats">
          <div className="reading"><span className="legend">{t("us.requests")}</span><strong>{fmt(stats.count)}</strong><span className="legend">{t("us.failedN", { n: stats.failed })}</span></div>
          <div className="reading"><span className="legend">{t("us.median")}</span><strong>{fmt(stats.latency)}<small>&nbsp;ms</small></strong><span className="legend">{t("us.firstToken", { v: fmt(stats.ttft, " ms") })}</span></div>
          <div className="reading"><span className="legend">{t("us.tokens")}</span><strong>{fmt(stats.input + stats.output)}</strong><span className="legend">{t("us.inOut", { in: fmt(stats.input), out: fmt(stats.output) })}</span></div>
        </div>
        <ul className="list">
          <li className="item usage-model legend" aria-hidden="true"><span>{t("us.colModel")}</span><span className="count">{t("us.requests")}</span><span className="count">{t("us.colFailed")}</span><span className="count">{t("us.colTokens")}</span></li>
          {stats.models.map((m) => <li className="item usage-model" key={m.model}>
            <code className="ellipsis">{m.model}</code><span className="num count">{m.count}</span>
            <span className="num count">{m.failed || ""}</span><span className="num count">{(m.input + m.output).toLocaleString()}</span>
          </li>)}
        </ul>
        <h3>{t("us.latest")}</h3>
        <ul className="list">{events.slice(-30).reverse().map((e, i) => <li className="item usage-event" key={`${e.id}:${i}`}>
          <span className={`lamp ${e.failed ? "bad" : "ok"}`} />
          <span className="legend">{time(e.at)}</span><code className="ellipsis grow">{e.model}</code>
          <span className="num">{e.latency} ms · {e.failed ? `HTTP ${e.status || "?"}` : t("us.tok", { n: e.input + e.output })}</span>
          <Link className="legend mono ellipsis" to="/logs/$id" params={{ id: e.id }} activeProps={{}} title={e.id}>{e.id}</Link>
        </li>)}</ul>
      </>}
    </section>
  </>;
}
