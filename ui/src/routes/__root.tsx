import { Button } from "../components/ui/button";
import { Input } from "../components/ui/input";
import { Link, Outlet, createRootRoute, useBlocker, useRouterState } from "@tanstack/react-router";
import { useQuery } from "@tanstack/react-query";
import { useEffect, useRef, useState, useSyncExternalStore, type FormEvent } from "react";
import { Rich } from "../components/Rich";
import { langs } from "../i18n";
import { getSnapshot as language, lang, setLang, subscribe as subscribeLang, t } from "../lang";
import { queries } from "../queries";
import { getSnapshot, pages, session, subscribe } from "../session";

function Mark() {
  return <><svg width="22" height="22" viewBox="0 0 40 40" aria-hidden="true">
    <rect width="40" height="40" rx="10" fill="var(--ink)" />
    <path d="M11 11h0M20 11h0M29 11h0M11 20h0M29 20h0M11 29h0M20 29h0M29 29h0"
      stroke="var(--ink-3)" strokeWidth="7" strokeLinecap="round" />
    <circle cx="20" cy="20" r="4.5" fill="var(--accent)" />
  </svg>cliproxy-rs</>;
}
function Theme() {
  return <Button variant="quiet" size="round" onClick={() => session.toggleTheme()}
    aria-label={t(session.theme === "dark" ? "app.themeLight" : "app.themeDark")}>
    <svg className="i" aria-hidden="true"><use href={`#i-${session.theme === "dark" ? "sun" : "moon"}`} /></svg>
  </Button>;
}
function Language() {
  const next = lang() === "ko" ? "en" : "ko";
  return <Button variant="quiet" size="small" onClick={() => setLang(next)} lang={next}
    aria-label={`${t("lang.label")}: ${langs[next]}`}>{langs[next]}</Button>;
}
function AppShell() {
  const state = useSyncExternalStore(subscribe, getSnapshot);
  const currentLanguage = useSyncExternalStore(subscribeLang, language);
  const pathname = useRouterState({ select: (s) => s.location.pathname });
  const page = pathname.split("/")[1] || "overview";
  const current = page === "connect" ? "credentials" : page;
  const name = page === "connect" ? "page.connect" : pages.find(([id]) => id === current)?.[1] || "page.overview";
  const [menu, setMenu] = useState(false);
  const [secret, setSecret] = useState("");
  const [failure, setFailure] = useState("");
  const [connecting, setConnecting] = useState(false);
  const menuButton = useRef<HTMLButtonElement>(null);
  const previousPage = useRef(page);
  const creds = useQuery({ ...queries.creds(), enabled: state.logged });
  const version = /^\d/.test(state.meta.version) ? `v${state.meta.version}` : state.meta.version;

  // Match the Svelte store.sync(): ask before an in-app navigation drops a dirty draft.
  // The beforeunload listener below owns browser close, so enableBeforeUnload stays off.
  useBlocker({
    shouldBlockFn: () => {
      if (!session.dirty) return false;
      if (confirm(t("app.discard"))) {
        session.dirty = false;
        return false;
      }
      return true;
    },
    enableBeforeUnload: false,
  });

  useEffect(() => {
    document.title = `${state.logged ? t(name) : t("app.signInTitle")} · cliproxy-rs`;
  }, [state.logged, name, currentLanguage]);
  useEffect(() => {
    if (previousPage.current === page) return;
    previousPage.current = page;
    setMenu(false);
    document.getElementById("main")?.focus({ preventScroll: true });
  }, [page]);
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape" && menu) {
        setMenu(false);
        menuButton.current?.focus();
      }
    };
    const onUnload = (event: BeforeUnloadEvent) => {
      if (session.dirty) event.preventDefault();
    };
    addEventListener("keydown", onKey);
    addEventListener("beforeunload", onUnload);
    return () => {
      removeEventListener("keydown", onKey);
      removeEventListener("beforeunload", onUnload);
    };
  }, [menu]);

  async function login(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setConnecting(true);
    setFailure("");
    try {
      await session.login(secret);
      setSecret("");
    } catch (error) {
      setFailure(error instanceof Error ? error.message : String(error));
    } finally {
      setConnecting(false);
    }
  }
  const toast = state.toast && <div className="toast" role={state.toast.bad ? "alert" : "status"}>
    <span className={`lamp ${state.toast.bad ? "bad" : "ok"}`} />{state.toast.text}
    {state.toast.bad && <Button variant="quiet" size="round" className="small" aria-label={t("app.dismiss")}
      onClick={() => session.dismiss()}><svg className="i" aria-hidden="true"><use href="#i-close" /></svg></Button>}
  </div>;

  if (!state.logged) return <>
    <main className="login"><form onSubmit={login}>
      <div className="top-row"><h1 className="brand"><Mark /></h1><span className="row"><Language /><Theme /></span></div>
      <div className="window">
        <label className="field">{t("app.managementKey")}
          <Input type="password" required value={secret} autoComplete="current-password"
            onChange={(event) => setSecret(event.target.value)} />
        </label>
        {failure && <p className="note error" role="alert"><span className="lamp bad" />{failure}</p>}
        <Button type="submit" variant="primary" disabled={connecting}>{connecting ? t("app.connecting") : t("app.connect")}</Button>
      </div>
      <p className="small muted"><Rich text={t("app.loginHint")} /></p>
    </form></main>{toast}
  </>;

  return <>
    <a className="key skip" href="#main" onClick={(event) => {
      event.preventDefault();
      document.getElementById("main")?.focus();
    }}>{t("app.skip")}</a>
    <div className="shell">
      <nav className={`side${menu ? " open" : ""}`} id="pages" aria-label={t("app.pages")}>
        <Link className="brand" to="/overview" activeProps={{}}><Mark /></Link>
        <div className="nav">
          {pages.map(([id, label], index) => label
            ? <Link key={id} to={`/${id}`} activeProps={{}}
                aria-current={current === id ? "page" : undefined}
                onClick={() => {
                  setMenu(false);
                  if (menu) menuButton.current?.focus();
                }}>
                {t(label)}{id === "credentials" && !!creds.data?.length && <small>{creds.data.length}</small>}
              </Link>
            : <hr key={`separator-${index}`} />)}
        </div>
        <div className="side-foot"><Theme /><Language />
          <Button variant="quiet" onClick={() => {
            if (!session.dirty || confirm(t("app.discard"))) session.logout();
          }}><svg className="i" aria-hidden="true"><use href="#i-out" /></svg>{t("app.signOut")}</Button>
        </div>
      </nav>
      <div className="main">
        <header className="top">
          <Link className="brand" to="/overview" activeProps={{}}><Mark /></Link>
          <span className="row" role="status">
            <span className={`lamp ${state.online ? "ok" : "bad"}`} />
            {state.online ? <><span className="sr">{t("app.connectedTo")}</span>
              <span className="host">{location.host}{version ? ` · ${version}` : ""}</span></> : t("app.lost")}
          </span>
          <Button ref={menuButton} size="round" className="menu" aria-label={t("app.pages")}
            aria-expanded={menu} aria-controls="pages" onClick={() => {
              setMenu(!menu);
              if (!menu) requestAnimationFrame(() => document.querySelector<HTMLElement>(".nav a")?.focus());
            }}><svg className="i" aria-hidden="true"><use href={`#i-${menu ? "close" : "menu"}`} /></svg></Button>
        </header>
        <main className="page" id="main" tabIndex={-1} inert={menu}><Outlet /></main>
      </div>
    </div>{toast}
  </>;
}

export const Route = createRootRoute({ component: AppShell });
