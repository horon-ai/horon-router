# CLIProxyAPI와의 호환성과 대조

> 이 문서에서 알 수 있는 것: horon-router가 CLIProxyAPI와 얼마나 같게 동작하는지, 항목별 대조 결과와 그 근거.

horon-router는 커밋 [`6fecc6e`](https://github.com/router-for-me/CLIProxyAPI/tree/6fecc6e5567912661654a4eaf9b8f5436facd1c2)(v8.0.10)의 CLIProxyAPI처럼 동작하는 것을 목표로 합니다. 같은 `config.yaml`, 같은 인증 파일, 같은 경로, 같은 응답입니다. 얼마나 가까운지 추적하려고 그 커밋의 모든 경로, 형식 변환, 설정 항목, 플러그인 메서드, Go 테스트 파일을 항목 하나씩으로 세웠습니다. 모두 1,687개이고, 각 항목을 이 저장소와 대조합니다.

<a id="what-works-today"></a>
## 지금 동작하는 것

- 클라이언트 API: `POST /v1/messages`와 `/v1/messages/count_tokens`(Anthropic), `POST /v1/chat/completions`, `/v1/completions`, `/v1/responses`, `/v1/responses/compact`(OpenAI), `/v1beta/models/...`와 `/v1beta/interactions`(Gemini), `GET /v1/models`, 그리고 `/backend-api/codex/` 아래의 Codex 경로입니다. 스트리밍(SSE: 서버가 결과를 조금씩 흘려 보내는 방식)과 비스트리밍을 모두 지원하고, 세 프로토콜 사이에서 형식을 변환합니다.
- 이미지와 영상: Codex 계정, xAI, OpenAI 호환 업스트림을 통한 `POST /v1/images/generations`와 `/v1/images/edits`, xAI 영상용 `/v1/videos`와 `/openai/v1/videos` 경로입니다.
- WebSocket: `GET /v1/responses`와 `GET /backend-api/codex/responses`의 Responses WebSocket입니다. Codex 클라이언트가 쓰며 응답 조향(`oauth.providers.codex.response-steering`)을 포함합니다.
- Codex 계정을 통한 실시간과 라이브: `/v1/realtime`(WebSocket과 WebRTC 호출), `/v1/live`, 콜 사이드밴드, 로컬 임시 키(`/v1/realtime/client_secrets`)입니다. WebRTC 미디어 릴레이는 선택 빌드 기능이고(`cargo build --release -p horonrouter --features hr-server/media-relay`) 릴리스 실행 파일에는 들어 있지 않습니다.
- 제공자: Claude(OAuth와 API 키), Codex(OAuth와 API 키), Kimi, Meta, xAI, Devin, Gemini API 키와 Gemini Interactions, Vertex AI(`-vertex-import`로 가져온 서비스 계정과 API 키), AI Studio(`/v1/ws` 브라우저 릴레이를 통함), OpenRouter 같은 OpenAI 호환 업스트림입니다.
- 명령줄이나 대시보드에서 하는 계정 로그인: Claude, Codex(브라우저 또는 기기 코드), Kimi, Meta, xAI, Devin입니다.
- 라우팅: 라운드 로빈, 가중치, fill-first 선택, 재시도, 쿨다운, 세션 고정, 모델 별칭과 제외, 페이로드 규칙, 인증 정보별 프록시와 전역 프록시입니다.
- 설정, 인증 정보, OAuth 로그인, 사용 한도 확인(`/requests/api-call`), 사용량 카운터, 로그, 모델 목록을 다루는 v8 관리 API와 `/management.html`의 대시보드입니다.
- 주 포트의 HTTPS(`server.tls`), Go 형식으로 stdout이나 교체되는 `main.log`에 남기는 로그와 Go의 요청별 접근 로그 줄, 클라이언트와 업스트림 구획이 있는 요청 로그 파일, LAN 검색(`-discover`와 `server.discovery` 광고), `.env` 읽기, Go처럼 하는 원격 모델 목록 갱신(`--local-model`을 주면 끕니다)입니다.
- 플러그인(`plugins`, Linux와 macOS): 로딩과 설정, 플러그인이 정의한 경로, 플러그인을 나열·활성화·설정·삭제하는 관리 API 경로, 플러그인 사용 한도 경로입니다.
- Home 모드(`-home-jwt`): bootstrap, 설정 갱신, 요청 분배, 사용량, 프로세스·요청 로그, 진행 중 보고, 공유 KV 상태입니다. Home이 관리하는 플러그인 동기화, 작업, 상태 보고는 아직 쓸 수 없습니다.
- `PGSTORE_*`, `OBJECTSTORE_*`, `GITSTORE_*` 저장소 백엔드와 주 포트의 Redis 프로토콜 사용량 구독자입니다.
- 설정과 인증 파일은 지켜보다가 바뀌면 다시 시작하지 않고 다시 읽습니다. 평문 관리 키는 Go처럼 처음 시작할 때 해시합니다.

서버에 없는 엔드포인트가 있으면 대시보드는 실패로 처리하지 않고 그 사실을 알려 줍니다. 할 수 없는 동작은 꺼 두고 이름을 밝히며, 열 수 없는 페이지는 어떤 경로가 없는지 말합니다. 아직 없는 것은 [앞으로 추가할 기능](../README.md#upcoming-features)에 있고, 일부러 다르게 만든 부분은 [DIFFERENCES-FROM-GO.md](DIFFERENCES-FROM-GO.md)에 있습니다.

<a id="where-it-stands"></a>
## 현황

<!-- parity-summary:start -->
점검일: 2026-10-05.

| 마일스톤 | 항목 수 | 충족 | 일부 충족 | 미구현 |
|---|---:|---:|---:|---:|
| M1 | 118 | 53 | 65 | 0 |
| M2 | 158 | 111 | 40 | 7 |
| M3 | 350 | 190 | 112 | 48 |
| M4 | 510 | 232 | 258 | 20 |
| M5 | 303 | 180 | 102 | 21 |
| M6 | 248 | 69 | 130 | 49 |
| 합계 | 1687 | 835 | 707 | 145 |
<!-- parity-summary:end -->

항목이 충족이라는 것은 구현되어 있고 Rust 테스트나 Go 서버에서 기록한 fixture(미리 저장해 둔 응답 자료)가 그것을 확인한다는 뜻입니다. 일부 충족은 동작하기는 하지만 모든 경우가 테스트로 고정되지 않았거나 일부만 구현되었다는 뜻입니다. 일부 충족 항목의 대부분은 Go 테스트 파일이고, 각 경우를 하나씩 옮기지 않아도 다른 테스트가 그 동작을 덮습니다. 미구현은 구현되지 않았다는 뜻입니다. [DIFFERENCES-FROM-GO.md](DIFFERENCES-FROM-GO.md)에 있는 의도한 차이는 충족으로 셉니다.

<a id="what-each-milestone-covers"></a>
## 각 마일스톤이 다루는 범위

| 마일스톤 | 범위 |
|---|---|
| M1 | Claude: OAuth 로그인, Messages 그대로 전달, 요청 형태 맞추기, 모델 목록, 클라이언트 키 |
| M2 | OpenAI Chat, Completions, Responses 형식과 Gemini, Interactions, Claude 사이의 형식 변환 |
| M3 | 나머지 제공자: Codex, Antigravity, AI Studio, Vertex, Gemini 키, Kimi, xAI, Meta, Devin, OpenAI 호환 업스트림 |
| M4 | 라우팅과 운영: 계정 선택, 쿨다운, 재시도, 페이로드 규칙, 별칭, 프록시, 로깅, 공유 설정 |
| M5 | 관리 API, WebSocket, 실시간과 라이브 음성, 접근 제어 |
| M6 | 플러그인, 터미널 UI, LAN 검색, Home(클러스터) 모드 |

미구현 항목은 대부분 M6(플러그인 런타임 통합과 Home 모드 일부)과 M3(Antigravity)에 있습니다. 플러그인 스토어와 터미널 UI는 있지만 일부 Go 경우는 아직 옮기지 않았습니다. 어떤 기능이 비어 있는지는 README의 [앞으로 추가할 기능](../README.md#upcoming-features) 목록이 요약합니다.

이 대조는 소스 훑기와, 저장해 둔 경로 탐침(probe)과 손으로 내린 판단을 합칩니다. 다시 돌리면 저장해 둔 탐침과 판단을 그대로 씁니다. 실제 제공자와의 호환성은 따로 시험해야 합니다. 수치는 런타임 연결보다 뒤처질 수 있습니다. 호스트 라이브러리에 구현된 메서드를 서버가 아직 쓰지 않을 수 있기 때문입니다. 위의 기능 설명은 서버의 실제 호출 경로를 따릅니다.

<a id="the-audit-files"></a>
## 대조 파일

수치의 근거는 모두 [`docs/parity-audit/`](parity-audit)에 있습니다.

- [`checklist.md`](parity-audit/checklist.md)는 모든 항목을 그 항목이 나온 Go 소스 링크와 함께 나열합니다.
- [`status.md`](parity-audit/status.md)는 항목마다 상태, 근거(그 항목을 덮는 Rust 코드와 테스트), 비고를 적습니다.
- [`manual.tsv`](parity-audit/manual.tsv)에는 코드를 읽고 내린 판단과 그 근거가 들어 있습니다.
- [`audit.py`](parity-audit/audit.py)는 `status.md`와 위의 표를 다시 만듭니다. [`probe.py`](parity-audit/probe.py)는 실행 파일을 켜고 체크리스트의 모든 경로에 인증 정보 없이 요청해 어떤 경로가 연결되어 있는지 봅니다. 결과는 `routes.json`에 있습니다.

바꾼 뒤에는 저장소 루트에서 `python3 docs/parity-audit/audit.py`를 실행해 둘 다 새로 고칩니다.
