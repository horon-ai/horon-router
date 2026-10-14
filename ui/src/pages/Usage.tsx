import { Button } from "../components/ui/button";
import { useEffect, useState, useSyncExternalStore } from "react";
import { useQuery } from "@tanstack/react-query";
import { api, missing, text } from "../api";
import { buckets, sumBuckets, sum, label, provider, cleanEvent, statsRows, usageStats, type Buckets, type Data, type Event } from "../core";
import { Grille } from "../components/Grille";
import type { Key } from "../i18n";
import { t, subscribe as subscribeLang, getSnapshot as language } from "../lang";
import { queries } from "../queries";
import { session } from "../session";

const apiKeysQuery = { queryKey: ["observability", "usage", "api-keys"], queryFn: (): Promise<Data> => api("/observability/usage/api-keys") };
const toBuckets = (rows: Data[]): Buckets => ({
  total: rows.map((b) => (b.success || 0) + (b.failed || 0)),
  failed: rows.map((b) => b.failed || 0),
});
// The cumulative token/cache table can group by any of the server's three dimensions.
const dims = ["model", "provider", "auth_index"] as const;
type Dim = (typeof dims)[number];
const dimLabel: Record<Dim, Key> = { model: "us.dim.model", provider: "us.dim.provider", auth_index: "us.dim.authIndex" };

export function Usage() {
  useSyncExternalStore(subscribeLang, language);
  useSyncExternalStore(session.subscribe, session.getSnapshot);
  const creds = useQuery({ ...queries.creds(), refetchInterval: 15_000 });
  const keys = useQuery({ ...apiKeysQuery, refetchInterval: 15_000 });
  const [live, setLive] = useState(false);
  const [events, setEvents] = useState<Event[]>([]);
  const [problem, setProblem] = useState("");
  const [dim, setDim] = useState<Dim>("model");
  const byDim = useQuery({
    queryKey: ["observability", "usage", "stats", dim],
    queryFn: (): Promise<Data> => api(`/observability/usage/stats?group_by=${dim}`),
    refetchInterval: 15_000,
  });
  const dimRows = statsRows(byDim.data);
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
  const merged = sumBuckets(sorted.map((r) => r.b));
  const max = Math.max(1, ...sorted.flatMap((r) => r.b?.total || []));
  const stats = usageStats(events);

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
    <section className="window display stats" aria-label={t("us.title")}>
      <div className="reading"><span className="legend">{t("us.requests")}</span>
        <strong>{stats.count.toLocaleString()}</strong><span className="legend">{t("us.live")}</span></div>
      <div className="reading"><span className="legend">{t("us.colFailed")}</span>
        <strong>{stats.failed.toLocaleString()}</strong><span className="legend">{t("us.live")}</span></div>
      <div className="reading"><span className="legend">{t("us.activeKeys")}</span>
        <strong>{sorted.length.toLocaleString()}</strong><span className="legend">{t("us.last200")}</span></div>
    </section>
    {problem && <p className="note error" role="alert"><span className="lamp bad" />{problem}</p>}
    <section className="section">
      <div className="section-head"><h2>{t("us.traffic")}</h2></div>
      <div className="window">
        {merged ? <>
          <Grille data={merged} max={max} />
          <p className="legend">{t("us.reported")}</p>
        </> : <p className="muted">{creds.error ? text(creds.error) : creds.data ? t("us.none") : t("common.loading")}</p>}
      </div>
      {keys.error && <p className="note"><span className={`lamp ${missing(keys.error) ? "off" : "bad"}`} />
        {missing(keys.error) ? t("us.keysNA") : text(keys.error)}</p>}
      <div className="window">
        <ul className="list">
          <li className="item usage-key legend" aria-hidden="true"><span>{t("page.keys")}</span>
            <span className="count">{t("us.requests")}</span><span className="count">{t("us.colFailed")}</span>
            <span className="count">{t("us.colLast")}</span></li>
          {sorted.map((r) => <li className="item usage-key" key={`${r.name}:${r.via}`}>
            <span className="name grow"><strong className="mono">{r.name}</strong><small>{r.via}</small></span>
            <span className="num count">{r.b ? sum(r.b.total).toLocaleString() : "—"}</span>
            <span className="num count">{r.b ? sum(r.b.failed).toLocaleString() : "—"}</span>
            <span className="num count legend">{t("common.notReported")}</span>
          </li>)}
          {!sorted.length && <li className="state"><p>{t("us.none")}</p></li>}
        </ul>
      </div>
      <p className="note">{t("us.ask")}</p>
    </section>
    <section className="section">
      <div className="section-head">
        <h2>{t("us.statsTitle")}</h2>
        <div className="seg" role="group" aria-label={t("us.groupBy")}>
          {dims.map((d) => <button key={d} type="button" aria-pressed={dim === d}
            onClick={() => setDim(d)}>{t(dimLabel[d])}</button>)}
        </div>
      </div>
      {byDim.error ? <p className="note"><span className={`lamp ${missing(byDim.error) ? "off" : "bad"}`} />
        {missing(byDim.error) ? t("us.statsNA") : text(byDim.error)}</p>
      : !byDim.data ? <div aria-busy="true"><div className="skel" /></div>
      : !dimRows.length ? <div className="state"><p>{t("us.statsNone")}</p></div>
      : <div className="window"><div className="scroll">
        <table className="usage-stats">
          <thead><tr>
            <th scope="col">{t(dimLabel[dim])}</th>
            <th scope="col">{t("us.colRequests")}</th>
            <th scope="col">{t("us.colFailed")}</th>
            <th scope="col">{t("us.colInput")}</th>
            <th scope="col">{t("us.colOutput")}</th>
            <th scope="col">{t("us.colReasoning")}</th>
            <th scope="col">{t("us.colCacheRead")}</th>
            <th scope="col">{t("us.colCacheCreation")}</th>
            <th scope="col">{t("us.colTotal")}</th>
            <th scope="col">{t("us.colQuality")}<small>complete / unclassified / inconsistent</small></th>
          </tr></thead>
          <tbody>{dimRows.map((r) => <tr key={r.key}>
            <th scope="row"><strong className="mono">{r.key || t("us.never")}</strong>
              {dim !== "provider" && r.providers.length > 0 && <small>{r.providers.join(", ")}</small>}</th>
            <td className="num">{r.requests.toLocaleString()}</td>
            <td className="num">{r.failed.toLocaleString()}</td>
            <td className="num">{r.input.toLocaleString()}</td>
            <td className="num">{r.output.toLocaleString()}</td>
            <td className="num">{r.reasoning.toLocaleString()}</td>
            <td className="num">{r.cacheRead.toLocaleString()}</td>
            <td className="num">{r.cacheCreation.toLocaleString()}</td>
            <td className="num">{r.total.toLocaleString()}</td>
            <td className="num">{`${r.complete} / ${r.unclassifiedQ} / ${r.inconsistent}`}</td>
          </tr>)}</tbody>
        </table>
      </div></div>}
      <p className="note">{t("us.qualityNote")}</p>
    </section>
  </>;
}
