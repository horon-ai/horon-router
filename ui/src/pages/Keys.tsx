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

// The icon sprite has no bin glyph; the canvas delete key is inlined at the .i stroke.
function Bin() {
  return <svg className="i" viewBox="0 0 24 24" aria-hidden="true">
    <path d="M4 7h16M9 7V5h6v2M6.5 7l1 13h9l1-13M10 11v6M14 11v6" />
  </svg>;
}

export function Keys() {
  useSyncExternalStore(subscribeLang, language);
  const state = useSyncExternalStore(subscribe, getSnapshot);
  const config = useQuery(queries.config());
  const [draft, setDraft] = useState("");
  const [reveal, setReveal] = useState(false);
  const [open, setOpen] = useState(false);
  const keys = readPath(config.data || {}, "access/api-keys", []) as string[];
  function add(event: React.SubmitEvent) {
    event.preventDefault();
    const key = draft.trim();
    if (keys.includes(key)) {
      session.notify(t("keys.exists"), true);
      return;
    }
    void session.act(() => replace("access/api-keys", keys, [...keys, key]), t("keys.added"))
      .then((ok) => { if (ok) { setDraft(""); setOpen(false); } });
  }
  return <>
    <div className="page-head">
      <div className="page-head-copy">
        <h1>{t("keys.title")}{!!keys.length && <span>{keys.length}</span>}</h1>
        <p className="page-sub">{t("keys.sub")}</p>
      </div>
      {!!keys.length && <Button variant="quiet" aria-pressed={reveal}
        onClick={() => setReveal(!reveal)}>{t(reveal ? "common.hideKeys" : "common.showKeys")}</Button>}
      <Button variant="primary" aria-expanded={open} onClick={() => setOpen(!open)}>
        <svg className="i" aria-hidden="true"><use href="#i-plus" /></svg>{t("keys.new")}
      </Button>
    </div>
    <Load res={config} what={t("what.clientKeys")}>{() =>
      <section className="section">
        <p className="note window"><span className="lamp off" /><span className="grow"><Rich text={t("keys.intro")} /></span></p>
        {open && <form className="form window" onSubmit={add}>
          <label className="field">{t("keys.new")}<Input type="password" autoComplete="off" required
            value={draft} onChange={(e) => setDraft(e.target.value)} /></label>
          <Button type="button" onClick={() => setDraft(newKey())}>{t("keys.generate")}</Button>
          <Button type="submit" variant="primary" disabled={state.busy}>
            <svg className="i" aria-hidden="true"><use href="#i-plus" /></svg>{t("keys.addKey")}</Button>
        </form>}
        {keys.length ? <ul className="list window">
          {keys.map((key, i) => <li className="item" key={`${key}-${i}`}>
            <code className="ellipsis">{reveal ? key : mask(key)}</code>
            <Button variant="quiet" size="round" aria-label={t("common.copy")}
              onClick={() => void session.act(() => navigator.clipboard.writeText(key), t("common.copied"))}>
              <svg className="i" aria-hidden="true"><use href="#i-copy" /></svg>
            </Button>
            <span className="grow" />
            <Button variant="danger" size="round" aria-label={t("common.remove")} disabled={state.busy}
              onClick={() => window.confirm(t("keys.confirmRemove")) &&
                void session.act(() => replace("access/api-keys", keys, keys.filter((_, n) => n !== i)),
                  t("keys.removed"))}><Bin /></Button>
          </li>)}
        </ul> : <div className="state">
          <div className="row"><span className="lamp warn" />{t("keys.none")}</div>
          <p>{t("keys.noneBody")}</p>
        </div>}
      </section>}</Load>
  </>;
}
