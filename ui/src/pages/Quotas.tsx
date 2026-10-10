import { Button } from "../components/ui/button";
import { useState, useSyncExternalStore } from "react";
import { useQuery } from "@tanstack/react-query";
import { credState, credName, provider, label, ago, otherSignals } from "../core";
import { check, checkAll, canCheck, hasSource, limits, quotaKey } from "../quota";
import { Load } from "../components/Load";
import { Meter } from "../components/Meter";
import { Missing } from "../components/Missing";
import { t, subscribe as subscribeLang, getSnapshot as language } from "../lang";
import { queries } from "../queries";
import { session } from "../session";

const actions: [string, string, string][] = [
  ["POST", "/requests/api-call", t("qt.miss.live")],
  ["POST", "/quota/fetch", t("qt.miss.live")],
  ["POST", "/routing/cooldown/reset", t("qt.miss.reset")],
];

export function Quotas() {
  useSyncExternalStore(subscribeLang, language);
  const state = useSyncExternalStore(session.subscribe, session.getSnapshot);
  const creds = useQuery({ ...queries.creds(), refetchInterval: 15_000 });
  useQuery(queries.plugins());
  const [checking, setChecking] = useState(false);
  const list = creds.data || [];

  async function all() {
    setChecking(true);
    try {
      await checkAll(list);
    } finally {
      setChecking(false);
    }
  }

  return <>
    <div className="head">
      <h1>{t("qt.title")}</h1>
      <Button disabled={checking || !list.some(canCheck)} onClick={() => void all()}>
        {checking ? t("common.checking") : t("qt.checkAll")}
      </Button>
    </div>
    <Missing actions={actions} />
    <p className="muted">{t("qt.intro")}</p>
    <Load res={creds} what={t("what.credentials")}>{(accounts) => <ul className="list">
      {accounts.length ? accounts.map((a) => {
        const s = credState(a, Date.now(), t);
        const q = limits(a);
        const signals = otherSignals(a.quota, q && "windows" in q ? q.windows : []);
        return <li className="stack quota-item" key={`${a.name}\u0000${a.auth_index}`}>
          <div className="item">
            <span className={`lamp ${s.lamp}`} />
            <span className="name grow"><strong>{credName(a)}</strong><small>{label(provider(a), t)} · {s.label}</small></span>
            <Button size="small" disabled={!canCheck(a)} onClick={() => void check(a)}>{t("qt.check")}</Button>
            <Button variant="quiet" size="small"
              disabled={state.busy || !a.auth_index || !a.cooldowns?.length || !session.can("POST", "/routing/cooldown/reset")}
              onClick={() => void session.call("POST", "/routing/cooldown/reset", { auth_index: a.auth_index }, t("cr.act.cleared"))}>
              {t("qt.reset")}
            </Button>
          </div>
          {q && "windows" in q && <>
            {q.windows.length ? q.windows.map((w, i) => <Meter w={w} key={i} />) : <p className="legend">{t("qt.noWindows")}</p>}
            <span className="legend">{!q.at ? "" : state.quota[quotaKey(a)]
              ? t("qt.checked", { ago: ago(q.at, Date.now(), t) })
              : signals.length ? "" : t("qt.observed", { ago: ago(q.at, Date.now(), t) })}</span>
          </>}
          {q && "error" in q && <p className="note error"><span className="lamp bad" />{q.error}</p>}
          {!!signals.length && <div className="chips">
            {signals.map(([k, v]) => <span className="chip" key={k}>{k}: {String(v)}</span>)}
            <span className="legend">{t("qt.observedLower", { ago: ago(a.quota.observed_at, Date.now(), t) })}</span>
          </div>}
          {!q && !signals.length && !hasSource(a) && <p className="legend">
            {t("qt.noSource", { provider: label(provider(a), t) })}
          </p>}
        </li>;
      }) : <li className="state"><p>{t("qt.none")}</p></li>}
    </ul>}</Load>
  </>;
}
