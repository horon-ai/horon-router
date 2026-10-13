import { Input } from "../components/ui/input";
import { Button } from "../components/ui/button";
import { useEffect, useRef, useState, useSyncExternalStore, type ChangeEvent, type FormEvent } from "react";
import { useQuery } from "@tanstack/react-query";
import { api, ApiError } from "../api";
import { label, type Data } from "../core";
import { Missing } from "../components/Missing";
import { t, subscribe as subscribeLang, getSnapshot as language } from "../lang";
import { keys, queries, queryClient } from "../queries";
import { session } from "../session";
import { Link } from "@tanstack/react-router";

// The canvas lists six sample providers; the real list is every provider this server can
// sign in to, plus any a plugin contributes. Dropping the others would remove real flows.
const builtIn = ["claude", "codex", "antigravity", "kimi", "kimi-ai", "xai", "devin", "meta", "command-code", "nous", "github-copilot"];
const forwarded = ["claude", "codex", "antigravity", "xai", "devin"];
const missingActions: [string, string, string][] = [
  ["GET", "/oauth/auth-url", t("cn.miss.signin")],
  ["POST", "/oauth/import", t("cn.miss.vertex")],
];
// The sign-in mode shown beside each provider.
const mode = (p: string) => p === "claude" ? t("cn.mode.oauthPaste")
  : p === "vertex" ? t("cn.mode.import")
  : p === "nous" ? t("cn.mode.oauth")
  : t("cn.mode.device");

export function Connect() {
  useSyncExternalStore(subscribeLang, language);
  const state = useSyncExternalStore(session.subscribe, session.getSnapshot);
  const plugins = useQuery(queries.plugins());
  const providers = [...new Set([...builtIn, ...(plugins.data || []).filter((p: Data) => p.supports_oauth).map((p: Data) => String(p.oauth_provider || p.id))])];
  const [gone, setGone] = useState<string[]>([]);
  const [current, setCurrent] = useState<Data | null>(null);
  const [status, setStatus] = useState("");
  const [problem, setProblem] = useState("");
  const [callback, setCallback] = useState("");
  const latest = useRef({ current, status });
  latest.current = { current, status };
  const finish = (next: string) => {
    setStatus(next);
    if (session.toast && !session.toast.bad) session.dismiss();
  };
  async function cancel() {
    const active = latest.current.current;
    if (!active) return false;
    try {
      const result = await api(`/oauth/session?state=${encodeURIComponent(active.state)}`, "DELETE");
      if (active === latest.current.current && result.cancelled !== false) finish("cancelled");
      return true;
    } catch (error) {
      session.notify(t("cn.notCancelled", { message: error instanceof Error ? error.message : String(error) }), true);
      return false;
    }
  }
  async function start(p: string) {
    if (latest.current.current && latest.current.status === "wait" && !(await cancel())) return;
    await session.act(async () => {
      const result = await api(`/oauth/auth-url?provider=${encodeURIComponent(p)}${forwarded.includes(p) ? "&is_webui=true" : ""}`).catch((error) => {
        if (error instanceof ApiError && error.code === "provider_not_found") {
          setGone((before) => [...before, p]);
          throw new Error(t("cn.unavailable", { provider: label(p, t) }));
        }
        throw error;
      });
      const url = new URL(result.url);
      if (url.protocol !== "https:") throw new Error(t("cn.unsafe"));
      setCurrent({ ...result, provider: p, started: Date.now() });
      setStatus("wait");
      setProblem("");
      setCallback("");
    });
  }
  useEffect(() => {
    if (!current || status !== "wait") return;
    const active = current;
    const timer = setInterval(async () => {
      const result = await api(`/oauth/status?state=${encodeURIComponent(active.state)}`).catch((error) => ({
        status: "error", error: error instanceof Error ? error.message : String(error),
      }));
      if (result.status === "wait" || latest.current.current !== active || latest.current.status !== "wait") return;
      setProblem(result.error || "");
      finish(result.status);
      if (result.status === "ok") {
        session.notify(t("cn.connected", { provider: label(active.provider, t) }));
        void queryClient.invalidateQueries({ queryKey: keys.creds });
      }
    }, 2000);
    return () => clearInterval(timer);
  }, [current, status]);
  function sendCallback(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!current) return;
    const key = current.provider === "command-code";
    void session.act(() => api("/oauth/callback", "POST", key
      ? { provider: "command-code", state: current.state, code: callback.trim() }
      : { provider: current.provider, redirect_url: callback.trim() }), t("cn.callbackSent"));
  }
  function vertex(event: ChangeEvent<HTMLInputElement>) {
    const file = event.currentTarget.files?.[0];
    event.currentTarget.value = "";
    if (!file) return;
    const form = new FormData();
    form.append("file", file);
    void session.act(async () => {
      await api("/oauth/import?provider=vertex", "POST", form).catch((error) => {
        if (!(error instanceof ApiError && error.code === "provider_not_found")) throw error;
        setGone((before) => [...before, "vertex"]);
        throw new Error(t("cn.vertexNA"));
      });
      await queryClient.invalidateQueries({ queryKey: keys.creds });
    }, t("cn.vertexDone"));
  }
  return <>
    <div className="page-head">
      <div className="page-head-copy">
        <h1>{t("page.connect")}</h1>
        <p className="page-sub">{t("cn.sub")}</p>
      </div>
    </div>
    <Missing actions={missingActions.map(([method, path], i) =>
      [method, path, t(i ? "cn.miss.vertex" : "cn.miss.signin")])} />
    <div className="connect-cols">
    <section className="section connect-providers">
      <h2>{t("cn.providerList")}</h2>
      <div className="provider-list">{providers.map((p) => {
        const active = current?.provider === p && status === "wait";
        const off = state.busy || !session.can("GET", "/oauth/auth-url") || gone.includes(p);
        const body = <><span className="name grow"><strong>{label(p, t)}</strong><small>{mode(p)}</small></span>
          <svg className="i" aria-hidden="true"><use href="#i-chevron" /></svg></>;
        return p === "vertex"
          ? <label key={p} className="provider-card" aria-disabled={off}>
              {body}
              <input className="file" type="file" accept=".json,application/json"
                disabled={off} onChange={vertex} />
            </label>
          : <button type="button" key={p} className={`provider-card${active ? " selected" : ""}`}
              aria-pressed={active} disabled={off} onClick={() => void start(p)}>{body}</button>;
      })}</div>
    </section>
    <section className="window flow" aria-live="polite">
      <div className="section-head"><h2>{current ? t("cn.signInTo", { provider: label(current.provider, t) }) : t("cn.signInWith")}</h2>
        {current && <span className="row"><span className={`lamp ${status === "ok" ? "ok" : status === "wait" ? "warn live" : status === "error" ? "bad" : "off"}`} />
          {status === "wait" ? t("cn.wait") : status === "ok" ? t("cn.ok") : status === "error" ? t("cn.fail") : t("cn.cancelled")}</span>}</div>
      {!current && <p className="muted">{t("cn.oauth")}</p>}
      {current && (status === "wait" ? <>
        <ol className="steps"><li>
          <span>{current.user_code ? t("cn.step1Code") : t("cn.step1Paste")}</span>
          {current.user_code && <code className="code-big">{current.user_code}</code>}
          <div className="row"><a className="key primary" href={current.url} target="_blank" rel="noopener noreferrer">
            <svg className="i" aria-hidden="true"><use href="#i-open" /></svg>{t("cn.open")}</a>
            <Button onClick={() => void session.act(() => navigator.clipboard.writeText(current.url), t("cn.linkCopied"))}>
              <svg className="i" aria-hidden="true"><use href="#i-copy" /></svg>{t("cn.copyLink")}</Button></div>
        </li>
          {current.flow !== "device" && <li>
            <span>{current.provider === "command-code" ? t("cn.pasteKey") : t("cn.pasteUrl")}</span>
            <form className="form" onSubmit={sendCallback}>
              <label className="field">{t("cn.callbackLabel")}
                <Input type={current.provider === "command-code" ? "password" : "url"} required value={callback}
                  onChange={(event) => setCallback(event.target.value)}
                  placeholder={current.provider === "command-code" ? t("cn.ccKey") : "https://localhost:1455/auth/callback?code=..."}
                  autoComplete="off" /></label>
              <Button type="submit" disabled={state.busy}>{t("cn.finish")}</Button>
            </form>
          </li>}
        </ol>
        <div className="row"><Button variant="quiet" onClick={() => void cancel()}>{t("cn.cancel")}</Button></div>
      </> : status === "ok"
        ? <><p>{t("cn.saved")}</p><div className="row"><Link className="key primary" to="/credentials" activeProps={{}}>{t("cn.viewCreds")}</Link></div></>
        : <>{problem && <p className="error">{problem}</p>}
          <div className="row"><Button onClick={() => void start(current.provider)}>{t("cn.again")}</Button></div></>)}
    </section>
    </div>
  </>;
}
