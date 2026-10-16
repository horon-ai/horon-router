import { Button } from "../components/ui/button";
import { useEffect, useState, useSyncExternalStore } from "react";
import { useQuery } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { clientModels } from "../api";
import {
  ago, buckets, credName, credState, flag, label, newKey, provider, readPath,
  strategy, sum, sumBuckets, type Data, type Lamp,
} from "../core";
import { Grille } from "../components/Grille";
import { Load } from "../components/Load";
import { Meter } from "../components/Meter";
import { getSnapshot as language, subscribe as subscribeLang, t } from "../lang";
import { queries, replace } from "../queries";
import { canCheck, checkAll, limits } from "../quota";
import { getSnapshot, session, subscribe } from "../session";

export function Overview() {
  useSyncExternalStore(subscribeLang, language);
  const state = useSyncExternalStore(subscribe, getSnapshot);
  const creds = useQuery({ ...queries.creds(), refetchInterval: 10_000 });
  const config = useQuery(queries.config());
  const list = creds.data || [];
  const rows = list.map((a) => ({ a, b: buckets(a), s: credState(a, Date.now(), t) }));
  const traffic = sumBuckets(rows.map((r) => r.b));
  const max = Math.max(1, ...rows.flatMap((r) => r.b?.total || []));
  const total = traffic ? sum(traffic.total) : 0;
  const failed = traffic ? sum(traffic.failed) : 0;
  const count = (lamp: Lamp) => rows.filter((r) => r.s.lamp === lamp).length;
  const groups = [...new Set(rows.map((r) => provider(r.a)))].map((p) => ({
    p, rows: rows.filter((r) => provider(r.a) === p),
  }));
  const routing = readPath(config.data || {}, "routing", {}) as Data;
  const fmt = (n: number) => n.toLocaleString();
  const keys = readPath(config.data || {}, "access/api-keys", []) as string[];
  const served = rows.some((r) => r.a.success || r.a.failed || (r.b && sum(r.b.total)));
  const [start, setStart] = useState(() => flag.get("start"));
  const [tested, setTested] = useState(() => flag.get("start-test") === "1");
  const [probe, setProbe] = useState<{ lamp: Lamp; text: string } | null>(null);
  const [checking, setChecking] = useState(false);
  const limited = rows.map((r) => ({ a: r.a, l: limits(r.a) })).filter((x) => x.l);
  const checkable = rows.some((r) => canCheck(r.a));

  useEffect(() => {
    if (start || !creds.data || creds.data.length) return;
    flag.set("start", "open");
    setStart("open");
  }, [start, creds.data]);

  async function createKey() {
    if (!await session.act(
      () => replace("access/api-keys", keys, [...keys, newKey()]),
      t("start.keyCreated"),
    )) return;
    // Query invalidation refreshes the configuration and advances the card.
    requestAnimationFrame(() => {
      if (document.activeElement === document.body)
        document.querySelector<HTMLElement>(".start .key.primary")?.focus();
    });
  }
  async function test() {
    setProbe({ lamp: "off", text: t("start.sending") });
    try {
      const ids = await clientModels(keys[0]);
      if (!ids.length) {
        setProbe({ lamp: "warn", text: t("start.noModels") });
        return;
      }
      setProbe({ lamp: "ok", text: t("start.works", { n: ids.length }) });
      setTested(true);
      flag.set("start-test", "1");
      if (served || (rows.length > 0 && (served || flag.get("start-tool") === "1")))
        session.notify(t("start.allSet"));
    } catch (error) {
      setProbe({ lamp: "bad", text: error instanceof Error ? error.message : t("api.noReach") });
    }
  }
  const steps: {
    done: boolean; title: string; text: string; action: string;
    to?: "/connect" | "/keys"; run?: () => void; off?: boolean;
  }[] = [
    { done: rows.length > 0, title: t("start.s1.title"), text: t("start.s1.text"),
      to: "/connect", action: t("ov.connect") },
    keys.length
      ? { done: !!served || flag.get("start-tool") === "1", title: t("start.s2.title"),
          text: t("start.s2.text"), to: "/keys", action: t("start.s2.create") }
      : { done: false, title: t("start.s2.title"),
          text: t("start.s2.text"),
          run: () => void createKey(), action: t("start.s2.create") },
    { done: !!served || tested, title: t("start.s3.title"), text: t("start.s3.text"),
      run: () => void test(), action: t("start.s3.action"), off: !keys.length },
  ];
  const left = steps.filter((step) => !step.done).length;
  const next = steps.findIndex((step) => !step.done);
  useEffect(() => {
    if (start !== "open" || left || !creds.data) return;
    flag.set("start", "done");
    setStart("done");
  }, [start, left, creds.data]);
  const setup = start === "open" && !!config.data && !!creds.data && creds.data.length === 0 && left > 0;

  async function checkLimits() {
    setChecking(true);
    try {
      await checkAll(list);
    } finally {
      setChecking(false);
    }
  }

  return <>
    <div className="page-head">
      <div className="page-head-copy">
        <h1>{t("ov.title")}</h1>
        <p className="page-sub">{t("ov.sub")}</p>
      </div>
      <Link className="key primary" to="/connect" activeProps={{}}>
        <svg className="i" aria-hidden="true"><use href="#i-plus" /></svg>{t("ov.connect")}
      </Link>
    </div>
    {setup && <section className="window start" aria-labelledby="start">
      <div className="section-head">
        <h2 id="start">{t("start.title")}<small>{t("start.done", { n: 3 - left })}</small></h2>
        <Button variant="quiet" size="small" onClick={() => {
          flag.set("start", "dismissed");
          setStart("dismissed");
          document.getElementById("main")?.focus();
        }}>{t("common.dismiss")}</Button>
      </div>
      <ol className="list">
        {steps.map((step, i) => <li className={step.done ? "done" : ""} key={i}>
          {step.done
            ? <svg className="i mark" aria-hidden="true"><use href="#i-check" /></svg>
            : <span className="lamp off" />}
          <span className="grow"><strong>{step.title}</strong>
            {step.done && <span className="sr">{t("start.srDone")}</span>}
            {i === 2 && probe
              ? <span className="note" role="status">
                  {!step.done && <span className={`lamp ${probe.lamp}${probe.text === t("start.sending") ? " live" : ""}`} />}
                  {probe.text}
                </span>
              : !step.done && <small>{step.text}</small>}
          </span>
          {step.done
            ? i === 0 && <span className="legend">{t("cn.ok")}</span>
            : step.to
              ? <Link className={`key small ${i === next ? "primary" : ""}`} to={step.to} activeProps={{}}>{step.action}</Link>
              : <Button size="small" variant={i === next ? "primary" : "default"}
                  disabled={state.busy || step.off} onClick={step.run}>{step.action}</Button>}
        </li>)}
      </ol>
    </section>}
    <Load res={creds} what={t("what.credentials")}>{(accounts) =>
      accounts.length > 0 && <>
        <section className="window display" aria-label={t("ov.last200")}>
          <div className="reading traffic">
            <span className="legend">{t("ov.requests")}</span>
            <strong>{traffic ? fmt(total) : "—"}</strong>
            {traffic
              ? <><Grille data={traffic} max={Math.max(1, ...traffic.total)} d={10} gap={6} stack={5} />
                  <span className="axis legend" aria-hidden="true"><span>{t("ov.minus200")}</span><span>{t("ov.now")}</span></span></>
              : <span className="legend">{t("ov.notReportedServer")}</span>}
          </div>
          <div className="reading">
            <span className="legend">{t("ov.ready")}</span>
            <strong>{count("ok")}<small>&nbsp;{t("ov.ofTotal", { n: accounts.length })}</small></strong>
            <span className="legend">{[
              count("warn") && t("ov.cooling", { n: count("warn") }),
              count("bad") && t("ov.failing", { n: count("bad") }),
              count("off") && t("ov.off", { n: count("off") }),
            ].filter(Boolean).join(" · ") || t("ov.allReady")}</span>
          </div>
          <div className="reading">
            <span className="legend">{t("ov.success")}</span>
            <strong>{traffic && total
              ? `${(((total - failed) / total) * 100).toFixed(total - failed === total ? 0 : 1)}`
              : "—"}<small>{traffic && total ? "%" : ""}</small></strong>
            <span className="legend">{!traffic ? t("common.notReported")
              : total ? t("ov.failedN", { n: fmt(failed) }) : t("ov.noRequests")}
              {traffic && !total && <span className="hint">{" · "}<Link to="/keys" activeProps={{}}>{t("ov.setupTool")}</Link></span>}
            </span>
          </div>
        </section>
        <p className="routing-line legend">{t("ov.routing")} <b>{t(strategy(routing.strategy).name).toLowerCase()}</b>
          {routing.retry?.["request-retry"] !== undefined && <> · {t("ov.retries", { n: routing.retry["request-retry"] })}</>}
          {routing["session-affinity"] && <> · {t("ov.affinity")}</>}
        </p>
        <section className="section">
          <div className="section-head">
            <h2>{t("ov.credentials")}</h2>
            {checkable && <Button variant="quiet" size="small" disabled={checking} onClick={() => void checkLimits()}>
              {checking ? t("ov.checking") : t("ov.check")}</Button>}
            <Link className="key quiet small" to="/credentials" activeProps={{}}>{t("common.manage")}
              <svg className="i" width="14" height="14" aria-hidden="true"><use href="#i-chevron" /></svg>
            </Link>
          </div>
          <ul className="list cards creds">
            {groups.flatMap((group) => [
              <li className="group" key={`${group.p}-heading`}>
                {label(group.p, t)}<span>· {group.rows.length}</span>
              </li>,
              ...group.rows.map((row) =>
              <li key={`${row.a.name}\u0000${row.a.auth_index}`}>
                <Link className="item" to="/credentials/$id" params={{ id: String(row.a.id || row.a.name) }} activeProps={{}}>
                  <span className={`lamp ${row.s.lamp}`} />
                  <span className="name grow"><strong>{credName(row.a)}</strong><small>{row.a.note || row.a.name}</small></span>
                  {row.b ? <Grille data={row.b} max={max} /> : <span className="legend">{t("ov.noHistory")}</span>}
                  <span className="num count">{row.b ? fmt(sum(row.b.total)) : "—"}</span>
                  <span className="state-label">{row.s.label}</span>
                </Link>
              </li>),
            ])}
            <li><Link className="item add" to="/connect" activeProps={{}}><span className="lamp off" />{t("ov.addAnother")}</Link></li>
          </ul>
        </section>
        {!!limited.length && <section className="section">
          <div className="section-head"><h2>{t("ov.limits")}</h2></div>
          <ul className="list cards limits">{limited.map(({ a, l }) => <li className="item" key={`${a.name}\u0000${a.auth_index}`}>
            <span className="name"><strong>{credName(a)}</strong><small>{label(provider(a), t)}
              {l && "at" in l && l.at ? ` · ${ago(l.at, Date.now(), t)}` : ""}
            </small></span>
            {l && "windows" in l
              ? <div className="windows">{l.windows.length
                  ? l.windows.map((window, i) => <Meter w={window} key={i} />)
                  : <span className="legend">{t("ov.noWindows")}</span>}</div>
              : l && <p className="note error grow"><span className="lamp bad" />{l.error}</p>}
          </li>)}</ul>
        </section>}
      </>
    }</Load>
  </>;
}
