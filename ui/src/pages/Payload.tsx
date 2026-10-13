import { Input } from "../components/ui/input";
import { Button } from "../components/ui/button";
import { useState, useSyncExternalStore, type FormEvent } from "react";
import { useQuery } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { readPath, type Data } from "../core";
import { Load } from "../components/Load";
import { t, subscribe as subscribeLang, getSnapshot as language } from "../lang";
import type { Key } from "../i18n";
import { queries, replace } from "../queries";
import { session } from "../session";

const kinds = ["default", "default-raw", "override", "override-raw", "filter"] as const;

export function Payload() {
  useSyncExternalStore(subscribeLang, language);
  const { busy } = useSyncExternalStore(session.subscribe, session.getSnapshot);
  const config = useQuery(queries.config());
  const [kind, setKind] = useState<(typeof kinds)[number]>("default");
  const [model, setModel] = useState("*");
  const [protocol, setProtocol] = useState("");
  const [paramsText, setParamsText] = useState('{\n  "temperature": 0.7\n}');
  const payload = readPath(config.data || {}, "requests/payload", {}) as Data;
  const rules = (payload[kind] || []) as Data[];

  function add(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    let params: unknown;
    try {
      params = JSON.parse(paramsText);
    } catch {
      session.notify(t("pl.badJson"), true);
      return;
    }
    const list = Array.isArray(params);
    if (kind === "filter" ? !list : list || !params || typeof params !== "object") {
      session.notify(kind === "filter" ? t("pl.filterList") : t("pl.object"), true);
      return;
    }
    const entry: Data = { name: model.trim() };
    if (protocol) entry.protocol = protocol;
    void session.act(
      () => replace(`requests/payload/${kind}`, rules, [...rules, { models: [entry], params }]),
      t("pl.added"),
    );
  }

  return <>
    <div className="page-head">
      <div className="page-head-copy">
        <h1>{t("pl.title")}</h1>
        <p className="page-sub">{t("pl.sub")}</p>
      </div>
      <Link className="key" to="/config" activeProps={{}}><svg className="i" aria-hidden="true"><use href="#i-edit" /></svg>{t("common.edit")}</Link>
    </div>
    <div className="seg" role="group" aria-label={t("pl.kind")}>
      {kinds.map((k) => <button key={k} aria-pressed={kind === k} onClick={() => setKind(k)}>
        {t(`pl.name.${k}` as Key)}<b>{(payload[k] || []).length || ""}</b>
      </button>)}
    </div>
    <Load res={config} what={t("what.payload")}>{() => <section className="section">
      <p className="muted">{t(`pl.${kind}` as Key)} {t("pl.match")}</p>
      {rules.length ? <ul className="list window">
        {rules.map((rule, i) => <li className="item rule" key={i}>
          <div className="stack grow">
            <div className="chips">{(rule.models || []).map((m: Data, n: number) =>
              <span className="chip" key={n}>{m.name}{m.protocol ? ` · ${m.protocol}` : ""}</span>)}</div>
            <pre className="legend">{JSON.stringify(rule.params, null, 2)}</pre>
          </div>
          <Button variant="danger" size="small" className="quiet" disabled={busy}
            onClick={() => void session.act(
              () => replace(`requests/payload/${kind}`, rules, rules.filter((_, n) => n !== i)),
              t("pl.removed"),
            )}>{t("common.remove")}</Button>
        </li>)}
      </ul> : <p className="note"><span className="lamp off" />{t("pl.noRules", { kind: t(`pl.name.${kind}` as Key) })}</p>}
      <form className="stack window" onSubmit={add}>
        <div className="form">
          <label className="field">{t("pl.model")}<Input required value={model} onChange={(e) => setModel(e.target.value)} /></label>
          <label className="field">{t("pl.protocol")}
            <select className="field" value={protocol} onChange={(e) => setProtocol(e.target.value)}>
              <option value="">{t("pl.any")}</option>
              {["openai", "claude", "gemini", "codex", "antigravity"].map((p) => <option key={p}>{p}</option>)}
            </select>
          </label>
        </div>
        <label className="field">{kind === "filter" ? t("pl.pathsList") : t("pl.valuesObj")}
          <textarea className="code" rows={5} spellCheck={false} value={paramsText}
            onChange={(e) => setParamsText(e.target.value)} />
        </label>
        <div className="row"><Button type="submit" variant="primary" disabled={busy}>
          <svg className="i" aria-hidden="true"><use href="#i-plus" /></svg>{t("pl.add")}
        </Button></div>
      </form>
    </section>}</Load>
  </>;
}
