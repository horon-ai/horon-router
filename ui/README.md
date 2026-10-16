# horon-router dashboard

The management dashboard for horon-router, written in React 19 with Tailwind CSS and shadcn-style Base UI components. It talks to the v8 Management API defined by Go CLIProxyAPI, and it has two outputs built from one source tree:

- `dist/` is embedded by the Rust binary (`crates/horon-server/build.rs`) and served at `/management.html`.
- `dist-panel/management.html` is one self-contained file for existing Go CLIProxyAPI servers. See [PANEL.md](PANEL.md).

Product rules are in [PRODUCT.md](PRODUCT.md) and the visual system in [DESIGN.md](DESIGN.md).

## Commands

Requires Node 22.12 or newer.

```sh
npm ci
npm run dev      # Vite on :5173, proxies /v8 to 127.0.0.1:8317 (override with CPA_BACKEND=...)
npm run check    # TypeScript, 0 errors expected
npm test         # unit tests for src/core.ts and src/i18n.ts
npm run build    # dist/, dist-panel/, then the size report
```

`npm run build` runs `vite build`, then `scripts/panel.mjs` (inlines JS, CSS, font and favicon into `dist-panel/management.html`, writes its SHA-256 to `dist-panel/management.html.sha256` and into PANEL.md), then `scripts/size.mjs`, which prints the gzip sizes of the JavaScript, CSS, HTML and single-file panel.

Rebuild the UI before compiling Rust when UI sources change; `ui/dist` is checked in and embedded at compile time.

## Structure

| Path | Role |
| --- | --- |
| `src/api.ts` | Fetch wrapper. Holds the management key in memory and reports unimplemented routes (Go-style empty 404, 501, 405). |
| `src/session.ts`, `src/queries.ts` | Session state and capability probes; TanStack Query owns server reads and reread-before-write mutations. |
| `src/core.ts` | Pure helpers: credential state, traffic buckets, usage records, quota windows, diffs. Tested in `src/core.test.ts`. |
| `src/i18n.ts`, `src/lang.ts`, `src/locales/` | Languages (English, 한국어). `t("key", { n })` looks up a message; the language comes from the saved choice (`localStorage` `horonrouter-lang`, set by the language button), else the first browser language we have, else English. Tested in `src/i18n.test.ts`. |
| `src/components/Load.tsx` | Loading, error, not-available and data states for one read. |
| `src/components/Missing.tsx` | One line naming actions this server does not implement. |
| `src/components/Grille.tsx` | The 20-bucket traffic grille. |
| `src/Editor.tsx` | JSON or YAML editor with a reviewed diff and a reread before writing. |
| `src/pages/*.tsx` | One component per screen. |
| `src/routes/` | TanStack Router hash routes that mount the screen components. |
| `src/components/ui/` | shadcn-style components built on Base UI and styled with Tailwind CSS. |

## Languages

Every sentence the dashboard shows is a key in `src/locales/en.ts`, with its Korean text under the same key in `src/locales/ko.ts`. `ko.ts` is typed from `en.ts`, so a missing key fails `npm run check`, and `npm test` also checks that both languages use the same `{placeholders}`. A message is a string, or `[one, other]` when English needs a plural picked by `{n}`; `\`backticks\`` render as code. Text that comes from the server (error messages, log lines) is shown as the server sent it. To add a language, add a file next to `ko.ts`, register it in `src/i18n.ts`, and write Korean-style plain words for people who are not developers.

## Honest states

Every read renders loading, empty, error, not available, or data. The server kind comes from response headers: horon-router sends `X-CPA-VERSION: horon-router-<version>`; Go sends its version with `X-CPA-COMMIT` and `X-CPA-BUILD-DATE`. Go implements the whole v8 API, so it is never probed.

Other servers are probed once per session for the write actions a screen offers. A route that does not exist answers with gin's empty 404 (or 501/405), and the UI cannot tell that apart from a real route without asking, because `OPTIONS` returns 204 everywhere. Each probe is therefore a request Go rejects with 400 during input validation, before any side effect: an empty JSON body to `POST /credentials`, `POST /credentials/refresh`, `PATCH /credentials/fields`, `DELETE /credentials`, `POST /routing/cooldown/reset`, `POST /requests/api-call` and `POST /oauth/import`; `GET /oauth/auth-url` without a provider; `GET /observability/usage/queue?count=0`. Each was checked against Go 6fecc6e to return 400 with the auth directory and config unchanged. Actions whose probe meets an empty 404 render disabled and are named in one line; reads the server lacks show "Not available on this server" with the route and status code. `DELETE /observability/logs` cannot be probed safely, so Clear is disabled when the log read is unavailable and otherwise marks itself off after a first empty 404.

Traffic comes from `recent_requests` in `GET /credentials` and `GET /observability/usage/api-keys` (twenty 10-minute buckets, reported by the server). The live usage view reads `GET /observability/usage/queue`, which removes records for other consumers, so it is opt-in, asks first, and keeps events in the tab only.

## Endpoints

All paths are under `/v8/management`. Every screen also uses `GET /config` and `GET /credentials`.

| Screen | Endpoints |
| --- | --- |
| Overview | `GET /credentials` every 10 s while visible |
| Credentials | `POST /credentials` (upload), `DELETE /credentials`, `GET /credentials/download`, `GET /credentials/models`, `PATCH /credentials/status`, `PATCH /credentials/fields`, `POST /credentials/refresh`, `POST /routing/cooldown/reset` |
| Connect | `GET /oauth/auth-url`, `GET /oauth/status` every 2 s while waiting, `POST /oauth/callback`, `DELETE /oauth/session`, `POST /oauth/import?provider=vertex` |
| Providers | `GET /observability/usage/api-keys`; `PUT /config/api-keys/<family>` |
| Client keys | `PUT /config/access/api-keys` |
| Models | `GET /routing/model-definitions/<channel>`; `PUT /config/oauth/model-alias/<channel>`, `PUT /config/oauth/excluded-models/<channel>` |
| Payload rules | `PUT /config/requests/payload/<kind>` |
| Quotas | `POST /requests/api-call` (Claude, Codex, Kimi usage), `POST /routing/cooldown/reset` |
| Usage | `GET /observability/usage/api-keys`, opt-in `GET /observability/usage/queue?count=500` |
| Logs | `GET /observability/logs` (cursor, every 3 s), `DELETE /observability/logs`, `GET /observability/logs/errors`, `GET /observability/logs/errors/<name>`, `GET /observability/logs/requests/<id>` |
| Configuration | `GET, PUT /config/<section>`, `GET, PUT /config.yaml` |
| System | `GET /server/latest-version` on request |

Config list writes reread the target first and refuse to write if it changed since it was shown. The API has no revision precondition, so this narrows the race window rather than closing it.

## Verification scripts

Both scripts are for disposable servers with fake credentials only: they edit configuration and credential files. Fixture requirements: management key `orb-dashboard-test-only`, a disabled fake Claude credential with email `operator@example.invalid`, routing strategy `round-robin`, and an empty `requests.payload.default`.

- `node scripts/browser-check-rust.mjs [url] [capture-dir]` drives the dashboard with agent-browser against the Rust binary (or Go): nine screens in both themes, unsupported actions disabled on Rust, a payload rule written and removed, a YAML diff discarded, 390 px layout, and no key in storage.
- `CHROME_PATH=... node scripts/panel-check-go.mjs [url] [capture-dir]` drives the single-file panel with Playwright against Go: all 13 screens in both themes, credential edits, a Codex OAuth start and cancel, config list writes, the section editor, request logs, the live usage view, phone layouts, and asserts that no request leaves the serving origin, Go is never probed, there are no console errors, and the key is not stored. `playwright-core` is a dev dependency for this script only; it downloads no browser.
- `node scripts/fake-logins.mjs [port]` serves fake provider login endpoints (Claude, Codex, Kimi, Meta token, profile, device-code and key-mint routes) and a Claude usage endpoint that echoes its `Authorization` header. Fake tokens and `@example.invalid` emails only.
- `scripts/login-harness/` is a test build of horon-router whose only change is `Options.login_base`, which points every provider login endpoint at the fake. It is never shipped. Build it with `cp ../../../Cargo.lock . && CARGO_TARGET_DIR=../../../target cargo build` inside that directory.
- `CHROME_PATH=... node scripts/oauth-check.mjs <url> --mode fake` drives Claude and Codex sign-in through a pasted callback URL and Kimi and Meta through the device code against the harness, and checks each new credential appears and that a provider the server lacks gets an honest message. `--mode dead` runs against the shipped binary with a dead `requests.proxy-url` and checks that a pasted callback ends in an honest failure.
- `node scripts/api-call-check.mjs <base> [<base> ...]` sends the Quotas screen's `POST /requests/api-call` to the fake usage endpoint through each server, checks `$TOKEN$` substitution and that the quota parser reads the windows, and compares the response shape between servers.
- `scripts/verify-backend.mjs` is the API-level verifier for the Go release backend, unchanged from the previous dashboard.

## Security

The management key lives in `src/api.ts` module memory and is cleared on sign-out; reloading signs out. Only the theme is stored. Downloaded credential files and configuration previews contain secrets; treat them accordingly. Do not use the dashboard over plain HTTP across a network.

## Fonts

Host Grotesk (UI) and JetBrains Mono (code), both SIL Open Font License in `public/fonts/OFL.txt`, subset to Latin with fontTools.
