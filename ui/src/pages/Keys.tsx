import { Input } from "../components/ui/input";
import { Button } from "../components/ui/button";
import { useState, useSyncExternalStore } from "react";
import { useQuery } from "@tanstack/react-query";
import { mask, newKey, readPath } from "../core";
import { Load } from "../components/Load";
import { Rich } from "../components/Rich";
import { getSnapshot as language, subscribe as subscribeLang, t } from "../lang";
import { queries, replace } from "../queries";
import { getSnapshot, session, subscribe } from "../session";

export function Keys() {
  useSyncExternalStore(subscribeLang, language);
  const state = useSyncExternalStore(subscribe, getSnapshot);
  const config = useQuery(queries.config());
  const [draft, setDraft] = useState("");
  const [reveal, setReveal] = useState(false);
  const keys = readPath(config.data || {}, "access/api-keys", []) as string[];
  function add(event: React.SubmitEvent) {
    event.preventDefault();
    const key = draft.trim();
    if (keys.includes(key)) {
      session.notify(t("keys.exists"), true);
      return;
    }
    void session.act(() => replace("access/api-keys", keys, [...keys, key]), t("keys.added"))
      .then((ok) => ok && setDraft(""));
  }
  return <>
    <div className="page-head">
      <div className="page-head-copy">
        <h1>{t("keys.title")}{!!keys.length && <span>{keys.length}</span>}</h1>
        <p className="page-sub">{t("keys.sub")}</p>
      </div>
      {!!keys.length && <Button variant="quiet" aria-pressed={reveal}
        onClick={() => setReveal(!reveal)}>{t(reveal ? "common.hideKeys" : "common.showKeys")}</Button>}
    </div>
    <Load res={config} what={t("what.clientKeys")}>{() =>
      <section className="section">
        <p className="muted"><Rich text={t("keys.intro")} /></p>
        {keys.length ? <ul className="list window">{keys.map((key, i) =>
          <li className="item" key={`${key}-${i}`}>
            <code className="grow ellipsis">{reveal ? key : mask(key)}</code>
            <Button size="small" onClick={() =>
              void session.act(() => navigator.clipboard.writeText(key), t("common.copied"))}>
              <svg className="i" width="14" height="14" aria-hidden="true"><use href="#i-copy" /></svg>{t("common.copy")}
            </Button>
            <Button variant="danger" size="small" className="quiet" disabled={state.busy}
              onClick={() => window.confirm(t("keys.confirmRemove")) &&
                void session.act(() => replace("access/api-keys", keys, keys.filter((_, n) => n !== i)),
                  t("keys.removed"))}>{t("common.remove")}</Button>
          </li>)}</ul> : <div className="state">
          <div className="row"><span className="lamp warn" />{t("keys.none")}</div>
          <p>{t("keys.noneBody")}</p>
        </div>}
        <form className="form window" onSubmit={add}>
          <label className="field">{t("keys.new")}<Input type="password" autoComplete="off" required
            value={draft} onChange={(e) => setDraft(e.target.value)} /></label>
          <Button type="button" onClick={() => setDraft(newKey())}>{t("keys.generate")}</Button>
          <Button type="submit" variant="primary" disabled={state.busy}>
            <svg className="i" aria-hidden="true"><use href="#i-plus" /></svg>{t("keys.addKey")}</Button>
        </form>
      </section>}</Load>
  </>;
}
