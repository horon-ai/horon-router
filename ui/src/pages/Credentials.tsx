import { Input } from "../components/ui/input";
import { Button } from "../components/ui/button";
import { useEffect, useMemo, useState, useSyncExternalStore, type ChangeEvent, type FormEvent } from "react";
import { useQuery } from "@tanstack/react-query";
import { useNavigate, useRouterState } from "@tanstack/react-router";
import { api, download } from "../api";
import { buckets, credName, credState, label, provider, span, sum, type Data } from "../core";
import { Grille } from "../components/Grille";
import { Load } from "../components/Load";
import { Missing } from "../components/Missing";
import { t, subscribe as subscribeLang, getSnapshot as language } from "../lang";
import { en } from "../locales/en";
import type { Key } from "../i18n";
import { keys, queries, queryClient } from "../queries";
import { session } from "../session";

const actions: [string, string, string][] = [
  ["POST", "/credentials", "upload"],
  ["POST", "/credentials/refresh", "token refresh"],
  ["PATCH", "/credentials/fields", "editing fields"],
  ["POST", "/routing/cooldown/reset", "cooldown reset"],
  ["DELETE", "/credentials", "delete"],
];
const id = (a: Data) => String(a.id || a.name);
const lookup = (a: Data) => ({ name: a.name, ...(a.auth_index ? { auth_index: a.auth_index } : {}) });
const said = (key: string, raw: string) => key in en ? t(key as Key) : raw;
const emptyFields = { note: "", priority: "", weight: "", request_retry: "" };
type Field = keyof typeof emptyFields;

export function Credentials({ credentialId }: { credentialId?: string }) {
  useSyncExternalStore(subscribeLang, language);
  const state = useSyncExternalStore(session.subscribe, session.getSnapshot);
  const navigate = useNavigate();
  const pathname = useRouterState({ select: (router) => router.location.pathname });
  const openId = pathname.startsWith("/credentials/")
    ? decodeURIComponent(pathname.slice("/credentials/".length))
    : credentialId;
  const creds = useQuery({ ...queries.creds(), refetchInterval: 10_000 });
  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState("all");
  const [fields, setFields] = useState(emptyFields);
  const [formFor, setFormFor] = useState("");
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), 10_000);
    return () => clearInterval(timer);
  }, []);
  const rows = (creds.data || []).map((a) => ({ a, b: buckets(a), s: credState(a, now, t) }));
  const max = Math.max(1, ...rows.flatMap((r) => r.b?.total || []));
  const selected = rows.find((r) => id(r.a) === openId)?.a;
  useEffect(() => {
    const next = selected ? id(selected) : "";
    if (next === formFor) return;
    setFormFor(next);
    if (selected) setFields({
      note: selected.note || "",
      priority: selected.priority === undefined ? "" : String(selected.priority),
      weight: selected.weight === undefined ? "" : String(selected.weight),
      request_retry: selected.request_retry === undefined ? "" : String(selected.request_retry),
    });
  }, [selected, formFor]);
  const models = useQuery({
    queryKey: ["credential-models", openId],
    queryFn: async (): Promise<Data[]> => (await api(`/credentials/models?name=${encodeURIComponent(openId!)}`)).models || [],
    enabled: !!selected,
  });
  const filters = ([
    ["all", "cr.f.all"], ["ok", "cr.f.ok"], ["warn", "cr.f.warn"], ["bad", "cr.f.bad"], ["off", "cr.f.off"],
  ] as const).map(([key, name]) => ({
    id: key, name: t(name), n: key === "all" ? rows.length : rows.filter((r) => r.s.lamp === key).length,
  })).filter((f) => f.id === "all" || f.n);
  const shown = rows.filter((r) => (filter === "all" || r.s.lamp === filter) &&
    `${r.a.name} ${credName(r.a)} ${provider(r.a)} ${r.a.note || ""}`.toLowerCase().includes(query.trim().toLowerCase()));
  const groups = [...new Set(shown.map((r) => provider(r.a)))].map((p) => ({ p, rows: shown.filter((r) => provider(r.a) === p) }));
  const missing = useMemo(() => actions.map(([method, path, name]) => [method, path, t(`cr.miss.${{
    upload: "upload", "token refresh": "refresh", "editing fields": "fields",
    "cooldown reset": "cooldown", delete: "delete",
  }[name]}` as Key)] as [string, string, string]), [language()]);
  const reload = () => queryClient.invalidateQueries({ queryKey: keys.creds });

  async function upload(event: ChangeEvent<HTMLInputElement>) {
    const files = [...(event.currentTarget.files || [])];
    event.currentTarget.value = "";
    if (!files.length) return;
    await session.act(async () => {
      const form = new FormData();
      files.forEach((file) => form.append("file", file));
      const result = await api("/credentials", "POST", form);
      await reload();
      if (result.failed?.length) throw new Error(t("cr.uploadedFailed", {
        n: result.uploaded || 0, list: result.failed.map((f: Data) => `${f.name} (${f.error})`).join(", "),
      }));
    }, files.length > 1 ? t("cr.uploadedMany", { n: files.length }) : t("cr.uploadedOne"));
  }
  function refreshAll() {
    void session.act(async () => {
      const result = await api("/credentials/refresh", "POST", { all: true });
      await reload();
      const bad = (result.results || []).filter((r: Data) => !r.success);
      if (bad.length) throw new Error(t("cr.refreshFailed", { n: bad.length, list: bad.map((r: Data) => `${r.id} (${r.error})`).join(", ") }));
      session.notify(result.results?.length ? t("cr.refreshedMany", { n: result.results.length }) : t("cr.refreshNone"));
    });
  }
  function saveFields(a: Data, event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const patch: Data = { name: id(a), note: fields.note.trim() };
    for (const k of ["priority", "weight", "request_retry"] as const) {
      const v = fields[k].trim();
      if (!v) {
        if (a[k] !== undefined) patch[k] = null;
        continue;
      }
      if (!/^-?\d+$/.test(v)) {
        session.notify(t("cr.wholeNumber", { field: k === "request_retry" ? t("cr.fRetries") : k === "priority" ? t("cr.fPriority") : t("cr.fWeight") }), true);
        return;
      }
      patch[k] = Number(v);
    }
    void session.call("PATCH", "/credentials/fields", patch, t("common.saved"));
  }
  const fieldNames: [Field, string, string][] = [
    ["note", t("cr.fNote"), ""], ["priority", t("cr.fPriority"), "0"],
    ["weight", t("cr.fWeight"), "1"], ["request_retry", t("cr.fRetries"), t("cr.fGlobal")],
  ];
  return <>
    <div className="head">
      <h1>{t("cr.title")}{!!rows.length && <span>{rows.length}</span>}</h1>
      <label className="key" aria-disabled={!session.can("POST", "/credentials")}>
        <svg className="i" aria-hidden="true"><use href="#i-upload" /></svg>{t("common.upload")}
        <input className="file" type="file" accept=".json,application/json" multiple
          disabled={state.busy || !session.can("POST", "/credentials")} onChange={upload} />
      </label>
      <Button disabled={state.busy || !rows.length || !session.can("POST", "/credentials/refresh")} onClick={refreshAll}>
        <svg className="i" aria-hidden="true"><use href="#i-refresh" /></svg>{t("cr.refresh")}
      </Button>
      <a className="key primary" href="#/connect"><svg className="i" aria-hidden="true"><use href="#i-plus" /></svg>{t("cr.connect")}</a>
    </div>
    <Missing actions={missing} />
    <Load res={creds} what={t("what.credentials")}>{(list) => !list.length
      ? <div className="state"><div className="row"><span className="lamp off" />{t("cr.none")}</div><p>{t("cr.noneBody")}</p></div>
      : <section className="section">
        <div className="row">
          <label className="search grow"><svg className="i" aria-hidden="true"><use href="#i-search" /></svg>
            <span className="sr">{t("cr.filterLabel")}</span>
            <Input value={query} onChange={(event) => setQuery(event.target.value)} placeholder={t("cr.filterPlaceholder")} />
          </label>
          <div className="seg" role="group" aria-label={t("cr.show")}>
            {filters.map((f) => <button key={f.id} aria-pressed={filter === f.id} onClick={() => setFilter(f.id)}>{f.name}<b>{f.n}</b></button>)}
          </div>
        </div>
        <ul className="list creds full">
          <li className="columns legend" aria-hidden="true"><span /><span>{t("cr.colCred")}</span><span>{t("cr.col200")}</span>
            <span className="count">{t("cr.colReq")}</span><span>{t("cr.colState")}</span></li>
          {groups.length ? groups.map((g) => <FragmentGroup key={g.p} name={label(g.p, t)} count={g.rows.length}>
            {g.rows.map((r) => {
              const a = r.a;
              const isOpen = openId === id(a);
              const facts: [string, unknown, boolean?][] = [
                [t("cr.fact.file"), a.name, true], [t("cr.fact.index"), a.auth_index || "—", true],
                [t("cr.fact.source"), a.runtime_only ? t("cr.src.config") : said(`cr.src.${a.source || "file"}`, a.source || "file")],
                [t("cr.fact.lifetime"), a.success === undefined ? t("common.notReported") : t("cr.life.ok", { ok: a.success, failed: a.failed || 0 })],
                [t("cr.fact.refreshed"), a.last_refresh && new Date(a.last_refresh).toLocaleString(t.lang)],
                [t("cr.fact.status"), a.status_message],
              ];
              const itemActions: [string, string, string, Data, string, string, boolean][] = [
                [a.disabled ? t("cr.act.enable") : t("cr.act.disable"), "PATCH", "/credentials/status", { ...lookup(a), disabled: !a.disabled }, a.disabled ? t("cr.act.enabled") : t("cr.act.disabled"), "", false],
                [t("cr.act.refresh"), "POST", "/credentials/refresh", lookup(a), t("cr.act.refreshed"), "", false],
                [t("cr.act.reset"), "POST", "/routing/cooldown/reset", { auth_index: a.auth_index }, t("cr.act.cleared"), t("cr.act.resetAsk"), !a.auth_index],
                [t("cr.act.delete"), "DELETE", "/credentials", { names: [a.name] }, t("cr.act.deleted"), t("cr.act.deleteAsk", { name: a.name }), !!a.runtime_only],
              ];
              return <li key={`${a.name}\0${a.auth_index}`} className={isOpen ? "open" : undefined}>
                <button className="item" aria-expanded={isOpen} onClick={() => void navigate({ to: isOpen ? "/credentials" : "/credentials/$id", params: isOpen ? undefined : { id: id(a) } } as Parameters<typeof navigate>[0])}>
                  <span className={`lamp ${r.s.lamp}`} />
                  <span className="name"><strong>{credName(a)}</strong><small>{a.note ? `${a.note} · ${a.name}` : a.name}</small></span>
                  {r.b ? <Grille data={r.b} max={max} /> : <span className="legend">{t("common.notReported")}</span>}
                  <span className="num count">{r.b ? sum(r.b.total).toLocaleString() : "—"}
                    {r.b && !!sum(r.b.failed) && <small className="error">{t("cr.failedN", { n: sum(r.b.failed) })}</small>}</span>
                  <span className="state-label">{r.s.label}{r.s.detail && <small title={r.s.detail}>{r.s.detail}</small>}</span>
                </button>
                {isOpen && <div className="detail window">
                  <dl className="facts">{facts.filter((f) => f[1]).map(([name, value, mono]) =>
                    <div key={name}><dt>{name}</dt><dd className={mono ? "mono" : undefined}>{String(value)}</dd></div>)}</dl>
                  {(!!a.cooldowns?.length || !!Object.keys(a.quota?.signals || {}).length) && <div className="chips">
                    {(a.cooldowns || []).map((c: Data, i: number) => <span className="chip" key={i}>
                      <span className="lamp warn" />{c.model_key || t("cr.credential")} · {said(`cr.why.${c.reason}`, String(c.reason ?? "").replaceAll("_", " "))}
                      {c.http_status ? ` ${c.http_status}` : ""} · {span(Date.parse(c.retry_at) - now, t)}</span>)}
                    {Object.entries(a.quota?.signals || {}).map(([k, v]) => <span className="chip" key={k}>{k}: {String(v)}</span>)}
                  </div>}
                  {!a.runtime_only && <form className="form" onSubmit={(event) => saveFields(a, event)}
                    inert={!session.can("PATCH", "/credentials/fields")}>
                    {fieldNames.map(([k, name, hint]) => <label className={`field${k === "note" ? " note-field" : ""}`} key={k}>
                      {name}<Input value={fields[k]} onChange={(event) => setFields({ ...fields, [k]: event.target.value })}
                        placeholder={hint} inputMode={k === "note" ? undefined : "numeric"} /></label>)}
                    <Button type="submit" disabled={state.busy || !session.can("PATCH", "/credentials/fields")}>{t("common.save")}</Button>
                  </form>}
                  <div className="stack"><h3>{t("cr.modelsH")}</h3>
                    {selected && <Load res={models} what={t("what.models")}>{(list) => <div className="chips">
                      {list.length ? list.map((m) => <span className="chip" key={m.id}>{m.id}</span>) : <span className="muted">{t("cr.noModels")}</span>}
                    </div>}</Load>}
                  </div>
                  <div className="row">
                    {itemActions.map(([name, method, path, body, done, ask, off]) => <Button key={`${method} ${path}`}
                      size="small" variant={method === "DELETE" ? "danger" : "default"} className={method === "DELETE" ? "quiet" : undefined}
                      disabled={state.busy || off || !session.can(method, path)}
                      onClick={() => void session.call(method, path, body, done, ask).then((ok) => {
                        if (ok && method === "DELETE") void navigate({ to: "/credentials" });
                      })}>{name}</Button>)}
                    <Button size="small" disabled={state.busy || a.runtime_only || !session.can("GET", "/credentials/download")}
                      onClick={() => void session.act(async () => download(
                        await api(`/credentials/download?name=${encodeURIComponent(a.name)}`, "GET", undefined, "blob"), a.name,
                      ))}><svg className="i" aria-hidden="true"><use href="#i-download" /></svg>{t("common.download")}</Button>
                  </div>
                </div>}
              </li>;
            })}
          </FragmentGroup>) : <li className="state"><p>{t("cr.noMatch")}</p></li>}
        </ul>
      </section>}
    </Load>
  </>;
}

import { Fragment, type ReactNode } from "react";
function FragmentGroup({ name, count, children }: { name: string; count: number; children: ReactNode }) {
  return <Fragment><li className="group">{name}<span>{count}</span></li>{children}</Fragment>;
}
