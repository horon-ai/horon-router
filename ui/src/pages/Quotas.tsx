import { Button } from "../components/ui/button";
import { useState, useSyncExternalStore } from "react";
import { useQuery } from "@tanstack/react-query";
import { api } from "../api";
import { credState, provider, label, span, type Data } from "../core";
import { check, checkAll, canCheck, limits } from "../quota";
import { Load } from "../components/Load";
import { Missing } from "../components/Missing";
import { t, subscribe as subscribeLang, getSnapshot as language } from "../lang";
import { keys, queries, queryClient } from "../queries";
import { session } from "../session";

const actions: [string, string, string][] = [
  ["POST", "/requests/api-call", t("qt.miss.live")],
  ["POST", "/quota/fetch", t("qt.miss.live")],
  ["POST", "/routing/cooldown/reset", t("qt.miss.reset")],
];
const cooling = (a: Data, now: number) =>
  (a.cooldowns || []).some((c: Data) => Date.parse(c.recover_at || c.retry_at) > now);
const Bar = ({ left }: { left: number }) =>
  <span aria-hidden="true" style={{ display: "block", height: "var(--space-2)", borderRadius: "var(--radius-key)", background: "var(--panel)", overflow: "hidden" }}>
    <span style={{ display: "block", height: "100%", borderRadius: "var(--radius-key)", background: "var(--ink)", width: `${left}%` }} />
  </span>;

export function Quotas() {
  useSyncExternalStore(subscribeLang, language);
  const state = useSyncExternalStore(session.subscribe, session.getSnapshot);
  const creds = useQuery({ ...queries.creds(), refetchInterval: 15_000 });
  const [checking, setChecking] = useState(false);
  const now = Date.now();
  const list = creds.data || [];

  async function all() {
    setChecking(true);
    try { await checkAll(list); } finally { setChecking(false); }
  }
  async function resetAll() {
    const targets = list.filter((a) => a.auth_index && cooling(a, now));
    if (!targets.length) return;
    await session.act(async () => {
      for (const a of targets) await api("/routing/cooldown/reset", "POST", { auth_index: a.auth_index });
      await queryClient.invalidateQueries({ queryKey: keys.creds });
    }, t("cr.act.cleared"));
  }

  return <>
    <div className="page-head">
      <div className="page-head-copy">
        <h1>{t("qt.title")}</h1>
        <p className="page-sub">{t("qt.sub")}</p>
      </div>
      <Button variant="primary" disabled={checking || !list.some(canCheck)} onClick={() => void all()}>
        <svg className="i" aria-hidden="true"><use href="#i-refresh" /></svg>
        {checking ? t("common.checking") : t("qt.checkAll")}
      </Button>
    </div>
    <Missing actions={actions} />
    <Load res={creds} what={t("what.credentials")}>{(accounts) => !accounts.length
      ? <div className="state"><div className="row"><span className="lamp off" />{t("qt.none")}</div></div>
      : <>
        <div className="family-grid">
          {accounts.flatMap((a) => {
            const s = credState(a, now, t);
            const q = limits(a);
            const windows = q && "windows" in q && q.windows.length ? q.windows : [null];
            return windows.map((w, i) => <article className="window stack quota-item" key={`${a.name}-${i}`}>
              <div className="row"><strong>{label(provider(a), t)}</strong><span className="grow" />
                <span className={`lamp ${s.lamp}`} /></div>
              {w ? <>
                <span className="legend">{w.label}</span>
                <strong className="num" style={{ fontSize: "var(--size-title)", letterSpacing: "-0.02em" }}>
                  {w.used === null ? (w.detail || "") : `${Math.round(100 - w.used)}%`}</strong>
                <Bar left={w.used === null ? 0 : Math.round(100 - w.used)} />
                <span className="legend">{w.detail && w.used !== null ? w.detail
                  : w.reset && Date.parse(w.reset) > now ? t("meter.resets", { span: span(Date.parse(w.reset) - now, t) }) : ""}</span>
              </> : <>
                <span className="legend">{s.label}</span>
                <strong className="num" style={{ fontSize: "var(--size-title)", letterSpacing: "-0.02em" }}>{t("common.notReported")}</strong>
                <Bar left={0} />
                <span className="legend">{t("qt.check")}</span>
              </>}
            </article>);
          })}
        </div>
        <section className="window">
          <div className="row">
            <span className="legend grow">{(() => {
              const soonest = Math.min(...accounts.filter((a) => cooling(a, now))
                .map((a) => Math.min(...(a.cooldowns || []).map((c: Data) => Date.parse(c.recover_at || c.retry_at)))));
              return Number.isFinite(soonest) ? t("cred.cooling", { span: span(soonest - now, t) }) : "";
            })()}</span>
            <Button variant="danger" disabled={state.busy || !accounts.some((a) => cooling(a, now)) || !session.can("POST", "/routing/cooldown/reset")}
              onClick={() => void resetAll()}>{t("qt.reset")}</Button>
          </div>
        </section>
      </>}</Load>
  </>;
}
