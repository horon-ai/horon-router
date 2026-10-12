# OpenAPI 명세와 문서 화면

> 이 문서에서 알 수 있는 것: 서버가 제공하는 OpenAPI 명세와 Scalar 문서 화면의 경로, 인증 경계, 명세에 포함되는 것과 빠지는 것, Scalar 문서 화면의 버전을 올리는 방법.

hyper-router 서버는 자기가 아는 HTTP 경로를 OpenAPI 3.1 명세로 내놓고, 그 명세를 Scalar 문서 화면으로 보여 줍니다. 화면은 서버가 직접 내장해 제공하므로 인터넷의 CDN이나 글꼴 서버를 부르지 않고, 화면의 시험 요청은 기본값으로 같은 서버(같은 출처)로 갑니다.

<a id="the-three-paths"></a>
## 제공하는 경로

| 경로 | 내용 |
|---|---|
| `GET /openapi.json` | 서버가 실행 중인 바이너리의 버전을 담은 OpenAPI 3.1 문서 |
| `GET /docs` | 그 문서를 보여 주는 HTML 페이지 |
| `GET /docs/assets/scalar.js` | 페이지가 쓰는 Scalar 자산 |

세 경로 모두 `GET`과 `HEAD`를 받습니다. `HEAD`는 같은 헤더(본문 길이 포함)를 돌려주고 본문은 비웁니다. 응답 헤더는 `Cache-Control: no-cache`이고, 내용에 맞는 `Content-Type`(HTML, JSON, JavaScript)을 붙입니다. 명세는 실행 중인 바이너리의 버전을 따르고 자산은 고정한 배포판을 따르므로, 캐시한 사본은 항상 다시 확인하도록 `no-cache`로 둡니다.

세 경로는 `/healthz`와 OAuth 콜백처럼 공개입니다. 클라이언트 키 검사를 하는 계층 뒤에 붙으므로 API 자체의 인증을 약하게 만들지 않고, 그 검사가 필요한 API 요청은 그대로 키를 요구합니다. 기본 포트는 8317이므로 로컬에서는 `http://127.0.0.1:8317/docs`로 엽니다.

`HEAD`는 이 세 경로와 `/healthz`만 받습니다. 그 밖의 경로에 `HEAD`를 보내면 등록되지 않은 경로와 같은 404입니다. 등록되지 않은 메서드(`POST /docs` 같은 것)나 없는 하위 경로(`/openapi.json/extra`)도 404이고, 405나 `Allow` 헤더는 붙지 않습니다. 이 규칙은 새 경로에만 적용하며 기존 경로의 `HEAD`와 404 동작은 바꾸지 않습니다.

정적 문서는 관리 패널 설정과 무관하게 제공합니다. `management.disable-control-panel: true`로 대시보드를 꺼도 `/docs`는 그대로 열리고, `access.api-keys`에 템플릿 값이 들어 있어 프록시 API를 막는 safe-mode에서도 세 경로는 200으로 남습니다. 화면은 키를 미리 채우지 않으며, 관리 키가 페이지에 새지 않습니다.

<a id="authentication-boundary"></a>
## 인증 경계

화면 자체는 공개지만, 화면에서 보내는 시험 요청은 서버의 기존 인증을 그대로 받습니다.

- 일반 API(고정 일반 경로와 `/v1`, `/v1beta` 등)는 설정한 클라이언트 키를 씁니다. `Authorization` 헤더(`Bearer <key>` 또는 키 그대로), `X-Goog-Api-Key` 헤더, `X-Api-Key` 헤더, `key` 쿼리 값, `auth_token` 쿼리 값 다섯 가지가 서로 대안(OR)입니다. 키를 설정하지 않으면 API는 열립니다. 실시간 통화와 사이드밴드 경로는 여기에 더해 `POST /v1/realtime/client_secrets`(또는 예전 `/v1/realtime/sessions`)가 발급하는 임시 `ek_...` 클라이언트 비밀값도 받습니다.
- 관리 API(`/v8/management`와 폐기 예정인 `/v0/management`)는 클라이언트 키와 다른 관리 키를 씁니다. `Authorization` 헤더(`Bearer <key>` 또는 키 그대로) 또는 `X-Management-Key` 헤더입니다. 관리 키가 없으면 이 트리가 인증 없이 열립니다(Go CLIProxyAPI는 이때 빈 본문 404로 답합니다). `GET`·`POST /v8/management/oauth/callback`(그리고 v0의 `oauth-callback`)은 키를 요구하지 않습니다.

관리 키는 `management.secret-key`로 설정하며 `ADMIN_PASSWORD` 환경 변수로도 줄 수 있습니다. 평문 값은 처음 시작할 때 bcrypt 해시로 바뀝니다. 자세한 내용은 [CONFIGURATION.md](CONFIGURATION.md#management)를 보십시오.

<a id="what-the-document-covers"></a>
## 명세가 다루는 범위

명세는 고정된 HTTP 계약을 담습니다.

- 고정 일반 API: `/healthz`, `/`, OAuth 콜백, OpenAI Chat Completions·Completions·Responses, Anthropic Messages, Gemini, 이미지, 영상, Codex, 실시간과 라이브, WebSocket 업그레이드 경로입니다.
- 관리 API: `/v8/management` 아래 고정 경로 전체와, Go의 v8 이전 표기인 `/v0/management` 경로입니다. v0 연산은 폐기 예정으로 표시(`deprecated`)합니다.
- 태그와 security scheme으로 일반 API와 관리 API를 구분합니다. 공개 경로는 `security: []`입니다.
- 제공자로 그대로 전달하는 요청·응답 본문은 필드를 고정하지 않고 여분 필드를 허용(`additionalProperties: true`)합니다. Gemini의 와일드카드 경로(`{*action}`)는 OpenAPI의 `{action}`으로 적고, 콜론 action과 여러 세그먼트 계약은 설명에 담습니다.
- 경로마다 `operationId`가 고유하고, 로컬 `$ref`가 모두 풀립니다.

명세에 넣지 않는 것도 분명히 합니다.

- 런타임에 플러그인이 등록하는 동적 HTTP 경로는 발명하지 않고 넣지 않습니다.
- WebSocket 메시지 본문과 RESP 프로토콜은 OpenAPI가 표현하는 HTTP 계약이 아니라서 넣지 않습니다.
- 정적 관리 패널 자산(`/ui`, `/management.html`, `/assets/*`, `/fonts/*`, `/favicon.svg`)은 파일이지 API 연산이 아니라서 경로로 넣지 않고 설명에만 적습니다.
- 문서 화면 자신의 경로(`/docs`, `/openapi.json`, `/docs/assets/scalar.js`)도 명세에 넣지 않습니다. `docs.rs`가 밝히듯, 정적 패널 경로와 같은 방식으로 제외합니다.
- 실제 인증 정보나 계정 데이터는 명세에 넣지 않습니다.

<a id="the-scalar-asset"></a>
## Scalar 자산

문서 화면은 npm 공식 배포의 Scalar standalone 번들을 그대로 내장해 씁니다.

| 항목 | 값 |
|---|---|
| npm 패키지 | `@scalar/api-reference` |
| 버전 | `1.73.1` (registry `dist-tags.latest`, 조회 시점 2026-10-11) |
| 라이선스 | MIT (`Copyright (c) 2023-present Scalar`) |
| 저장 위치 | `crates/hr-server/assets/scalar/scalar-standalone.js` |
| SHA-256 | `62c34a0dda38945864011ebaf6e7fa7812a332429120d18922b34d01c4f7ccc8` |

번들은 자체 완결형(IIFE)이라 외부 청크나 상대 경로 import가 없고, CSS도 번들 안에 들어 있습니다. 그래서 CDN이나 글꼴 서버, 대시보드 빌드 의존성 없이 오프라인에서 동작합니다.

페이지는 `Scalar.createApiReference`를 이렇게 설정합니다. `url: '/openapi.json'`과 `servers: [{ url: '/' }]`로 시험 요청의 기본 대상이 같은 서버(같은 출처)가 되게 하고, `persistAuth: false`로 인증 값을 브라우저 저장소에 남기지 않습니다. `telemetry: false`로 사용 정보를 보내지 않고, `withDefaultFonts: false`로 외부 글꼴을 부르지 않습니다. `agent: { disabled: true }`, `proxyUrl: ''`, `mcp: { disabled: true }`로 Scalar의 에이전트·프록시·MCP 경로를 끄고, `showDeveloperTools: 'never'`로 개발자 도구를 감춥니다. 그래서 Scalar가 스스로 키나 사용 정보를 서버 밖으로 내보내지 않습니다. 다만 사용자가 화면에서 서버 주소를 직접 바꾸면 그 주소로 요청을 보낼 수 있습니다.

<a id="updating-the-asset"></a>
## Scalar 문서 화면의 버전 올리기

화면이 쓰는 Scalar 번들의 버전을 올릴 때는 코드가 아니라 자산과 그 기록을 바꿉니다. 명세의 내용 자체는 `crates/hr-server/src/openapi.rs`와 `crates/hr-server/src/openapi/management.rs`가 코드로 만들며, 이 절차와 별개입니다.

1. `curl -sS https://registry.npmjs.org/@scalar/api-reference`로 `dist-tags.latest`를 확인합니다.
2. 그 버전 tarball을 내려받아 sha512(base64)와 sha1을 registry `dist` 값과 대조합니다.
3. `tar xzf <tarball> package/dist/browser/standalone.js`로 바이트를 그대로 뽑아 `crates/hr-server/assets/scalar/scalar-standalone.js`를 덮어씁니다.
4. 새 버전의 LICENSE 원문을 https://raw.githubusercontent.com/scalar/scalar/main/LICENSE 에서 받아 대조하고, `crates/hr-server/assets/scalar/PROVENANCE.md`의 버전·해시·크기 표를 갱신합니다.
5. `window.Scalar.createApiReference` 노출과 자체 완결성(외부 청크 참조 없음)을 다시 확인합니다.
6. `crates/hr-server/tests/openapi.rs`의 `PINNED_ASSET_SHA256` 상수를 새 번들의 SHA-256으로 갱신합니다. 이 값이 저장한 바이트와 다르면 테스트가 실패합니다.

이 갱신에는 새 npm 의존성이나 빌드 단계가 필요하지 않습니다. 번들은 Rust 빌드 시 `include_bytes!`로 실행 파일에 내장되고, 테스트가 그 바이트를 `PINNED_ASSET_SHA256`과 대조합니다. 출처와 무결성 기록의 원본은 `crates/hr-server/assets/scalar/PROVENANCE.md`입니다.

<a id="verifying-in-a-browser"></a>
## 브라우저에서 확인하기

실제 화면 동작은 격리한 `auth-dir`와 로컬 모의 업스트림으로 서버를 켜고 브라우저로 확인합니다. 확인할 것은 문서 로딩, 검색과 모델·태그 탐색, 인증 값 입력, 같은 서버의 `/healthz` 시험 요청과 거절 응답, 데스크톱과 모바일 화면, 밝은·어두운 화면, 키보드 이동입니다. 문서와 시험 요청이 서버 밖으로 나가지 않는지는 외부 요청 기록으로 확인합니다. 실제 제공자와 실제 계정은 쓰지 않습니다.
