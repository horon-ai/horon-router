# CLIProxyAPI(Go)에서 HoronRouter로 옮기기

> 이 문서에서 알 수 있는 것: Go로 만든 CLIProxyAPI에서 horon-router로 옮기는 방법, 그대로 넘어오는 것과 넘어오지 않는 것, 그리고 되돌리는 방법.

horon-router는 CLIProxyAPI의 커밋 `6fecc6e`(v8.0.10)를 기준으로 삼습니다. 같은 파일을 읽으므로, 옮기는 일은 실행 파일 하나를 멈추고 같은 설정으로 다른 실행 파일을 켜는 것으로 끝납니다. 이 문서는 그대로 넘어오는 것과 넘어오지 않는 것, 그리고 되돌리는 방법을 정리합니다.

<a id="what-carries-over"></a>
## 그대로 넘어오는 것

- `config.yaml`. v8 형식과 예전의 평평한 형식(최상위 `port`, `auth-dir`, `api-keys` 목록, `remote-management`)을 모두 읽습니다. 모든 v8 설정을 받습니다. horon-router에 아직 없는 기능의 설정(아래 목록)도 받습니다. 이런 설정은 파일에 남지만 효과가 없습니다.
- `oauth.auth-dir`(기본값 `~/.cli-proxy-api`)의 인증 파일. Claude, Codex, Kimi, Meta, xAI, Devin, Vertex, AI Studio 파일은 그대로 씁니다. Antigravity 파일은 horon-router가 아직 처리하지 않는 제공자의 파일입니다. 대시보드에는 표시되지만 요청에 쓰이지는 않습니다.
- 해시된 관리 키. Go와 horon-router 모두 시작할 때 평문 `management.secret-key`를 bcrypt로 해시해 다시 씁니다. 그리고 서로의 해시를 받아들입니다.
- `access.api-keys`의 클라이언트 키, `api-keys` 아래의 제공자 키, OpenAI 호환 업스트림, 모델 별칭과 제외 목록, 페이로드 규칙, 프록시, 라우팅 방식, 재시도, 쿨다운.
- 클라이언트 경로(`/v1/...`, `/v1beta/...`, `/backend-api/codex/...`)와 Responses WebSocket. 그래서 클라이언트는 바꿀 것이 없습니다.
- `/v8/management`의 v8 관리 API(아래 빈틈은 제외)와 `/management.html`의 대시보드.

대시보드나 관리 API로 설정을 바꾸면 horon-router는 그 키만 고치고 나머지 글과 주석은 그대로 둡니다. Go와 마찬가지로 그 첫 저장은 예전 평평한 설정을 v8 형식으로 바꾸고, v8 설정이 아닌 키를 지우지 않고 주석으로 옮깁니다. 대시보드의 원본 편집기는 내가 쓴 글을 그대로 저장합니다.

되돌리는 길도 됩니다. 가짜 로그인 서버를 상대로 한 시험에서 Go 6fecc6e는 horon-router가 쓴 Claude, Codex, Kimi, Meta 인증 파일을 활성으로 표시했고, horon-router가 해시한 관리 키도 받아들였습니다.

<a id="switching"></a>
## 옮기기

1. Go 서버를 멈춥니다.
2. `config.yaml`과 인증 파일 폴더를 백업합니다. 이 안에 OAuth 갱신 토큰이 들어 있으니 백업은 사설로 보관하십시오.
3. 아래 플래그 목록과 내 명령줄을 대조합니다. 몇 가지 Go 모드는 아직 쓸 수 없습니다. 그런 모드를 지정하면 horon-router는 그냥 시작하지 않고 오류를 내고 끝납니다.
4. 같은 설정으로 horon-router를 시작합니다. `horonrouter --config /path/to/config.yaml`입니다.
5. `/management.html`을 열어 인증 정보가 목록에 있고 정상인지 확인한 뒤, 클라이언트 키 하나로 시험 요청을 보냅니다.

systemd 서비스라면 실행 파일만 바꾸고 설정 경로와 인증 파일 경로는 그대로 두십시오. 기본 Docker 이미지는 `/data/config.yaml`을 쓰고 UID/GID 10001로 실행됩니다. 마운트와 권한을 고치거나, 기존 경로를 유지하려면 `--config`를 덮어쓰고 접근 가능한 `oauth.auth-dir`를 지정하십시오. 볼륨 소유자, 수신 주소, 관리 접근은 [INSTALL.md](INSTALL.md#docker)에서 다룹니다.

**주의:** 같은 계정의 인증 정보로 Go와 horon-router를 동시에 실행하지 마십시오. 인증 파일 폴더를 따로 복사했더라도 마찬가지입니다. 둘 다 OAuth 토큰을 갱신합니다. 갱신 토큰을 돌려 쓰는 제공자는 다른 서버가 가진 토큰을 무효로 만들고 다시 로그인하게 할 수 있습니다. 복사한 인증 파일 폴더는 격리된 시험이 아닙니다. 두 서버를 나란히 시험하려면 별도의 시험 계정과 다른 포트를 쓰십시오. 백업은 사설로 보관하고, 어느 한쪽이 갱신한 뒤에도 그 백업의 갱신 토큰이 쓸 수 있으리라고 가정하지 마십시오.

<a id="command-line-flags"></a>
## 명령줄 플래그

horon-router는 CLIProxyAPI의 모든 플래그를 받습니다. Go 방식의 한 개 대시 표기(`-config`)와 두 개 대시 표기를 모두 씁니다. 다음은 Go와 똑같이 동작합니다. `-config`, `-claude-login`, `-codex-login`, `-codex-device-login`, `-kimi-login`, `-kimi-ai-login`, `-xai-login`, `-meta-login`, `-devin-login`, `-vertex-import`(`-vertex-import-prefix`와 함께), `-no-browser`, `-oauth-callback-port`, `-password`, `--local-model`, `-home-jwt`(또는 `HOME_JWT`), 그리고 `-discover`(또는 `discover` 하위 명령)와 그 `-discover-*` 옵션을 쓰는 LAN 검색입니다.

`-antigravity-login`은 "not supported by horon-router yet" 오류와 상태 1로 끝납니다. `-tui`와 `-tui -standalone`은 Go와 같이 터미널 UI를 엽니다. 차이는 [DIFFERENCES-FROM-GO.md](DIFFERENCES-FROM-GO.md)에 있습니다.

<a id="not-available-yet"></a>
## 아직 쓸 수 없는 것

다음 설정은 `config.yaml`에서 받고 저장할 때도 남지만, horon-router는 아직 그것으로 아무것도 하지 않거나 일부만 합니다.

| Go 설정 또는 기능 | horon-router에서 |
| --- | --- |
| `pprof` | 프로파일링 엔드포인트가 없습니다. |
| 요청 경로의 플러그인 | 프런트엔드 인증, 모델 라우터와 그것이 연결하는 플러그인 실행기, 요청·응답·스트림 조각 인터셉터, 요청 수명 주기와 사용량 플러그인은 Go와 같이 동작합니다. `host.http.*`, `host.auth.*`, `host.affinity.lookup` 콜백도 마찬가지입니다. 플러그인이 소유한 제공자는 아직 처리하지 않습니다. 플러그인이 파싱하는 인증 파일(플러그인 로그인으로 저장한 파일 포함)은 인증 정보로 읽지 않고, 플러그인 모델과 실행기는 모델 라우터로만 닿습니다. 플러그인 스케줄러, 요청·응답 변환기, thinking 적용기, `host.model.*` 콜백, WebSocket 응답 관찰자는 호출되지 않습니다. |
| Home이 관리하는 플러그인 | `-home-jwt`를 쓰면 bootstrap, 설정 갱신, 요청 분배, 사용량, 프로세스·요청 로그, 진행 중 보고, 공유 KV 상태가 동작합니다. 플러그인 실행 파일은 미리 로컬에 설치되어 있어야 합니다. Home이 보낸 설정으로 그것을 설정할 수 있지만, Home이 관리하는 플러그인 동기화, 작업, 상태 보고는 쓸 수 없습니다. |
| `management.panel-github-repository`, `management.disable-auto-update-panel`, `MANAGEMENT_STATIC_PATH` | 대시보드는 실행 파일에 들어 있고 절대 내려받거나 디스크에서 읽지 않습니다. `management.disable-control-panel`은 적용됩니다. |
| 설정 재읽기 로그 요약 | 설정은 다시 읽지만 바뀐 내용을 로그에 요약하지 않습니다. |

작업 폴더의 `.env`는 Go와 같이 읽습니다. `PGSTORE_*`, `OBJECTSTORE_*`, `GITSTORE_*` 저장소 백엔드와 `WRITABLE_PATH`도 Go와 같이 동작합니다. `RUST_LOG`를 설정하면 `debug`에서 오는 로그 단계를 덮어씁니다.

아직 쓸 수 없는 제공자는 Antigravity 하나이고, README의 [앞으로 추가할 기능](../README.md#upcoming-features)에 있습니다.

<a id="management-api-differences"></a>
## 관리 API 차이

설정, 인증 정보, OAuth 로그인, `requests/api-call`, 쿨다운 초기화, 사용량, 로그, 모델 정의, 플러그인을 다루는 v8 경로는 Go와 같이 동작합니다. 예전 `/v0/management` 경로도 아래에 나온 것을 빼면 마찬가지입니다. API를 통한 OAuth 로그인은 Claude, Codex, Kimi, Meta, xAI, Devin에서 됩니다. Antigravity 로그인과 Vertex 가져오기(`oauth/import`)는 `404`와 `provider_not_found`를 돌려줍니다.

horon-router에서 쓸 수 없는 것:

- `server/latest-version`은 "no release repository is configured"와 함께 `502`를 돌려줍니다. horon-router가 아직 자기 릴리스를 확인하지 않기 때문입니다. Go는 GitHub에 최신 CLIProxyAPI 릴리스를 묻습니다.

함께 들어 있는 대시보드는 어느 서버와 대화 중인지 확인하고, 이런 기능을 실패로 처리하지 않고 쓸 수 없음으로 표시합니다.

<a id="differences-from-go"></a>
## Go와 다른 점

몇 가지 동작은 일부러 Go와 다릅니다. 예를 들어 설정을 쓸 때 `config.yaml`의 나머지 부분을 바이트 그대로 두고, 대시보드는 절대 내려받지 않습니다. 전체 목록은 [DIFFERENCES-FROM-GO.md](DIFFERENCES-FROM-GO.md)에 있습니다.

<a id="additions"></a>
### 추가 기능

Go에 없는 기능입니다. 모두 켜야 동작하고 기본값은 Go처럼 동작합니다.

- 초기화 시각을 보는 라우팅: `routing.strategy: soonest-reset`(별칭 `reset-first`)입니다. 준비된 계정 중에서 주간 창이 가장 먼저 초기화되는 계정을 고릅니다. 그 계정이 쿨다운에 들어가거나 사용 한도에 닿을 때까지 그대로 쓰고, 그러면 다음 계정으로 넘어갑니다. 초기화 시각은 각 계정의 최신 응답 헤더에서 가져옵니다(Claude는 `anthropic-ratelimit-unified-*`, Codex는 `x-codex-*`). 앞으로의 초기화 시각을 모르는 계정은 요청 하나를 보내 알아냅니다. 초기화 헤더가 없는 계정은 초기화 시각을 아는 계정 다음으로 갑니다. 같은 순위의 계정은 번갈아 씁니다. 세션 고정이 우선합니다. 한 모델에 제공자가 여럿이면 `fill-first`와 마찬가지로 첫 제공자를 고릅니다. 이 방식은 실험적이고 켜야 동작합니다. 기본값은 여전히 `round-robin`이고, Go는 `soonest-reset`을 `round-robin`으로 읽습니다.
- Codex 연결 재사용: `oauth.providers.codex.chatgpt-keep-alive: true`로 설정하면 프록시마다 `chatgpt.com`에 놀고 있는 연결을 최대 2개까지 90초 동안 유지합니다. 요청마다 새로 열지 않습니다. 기본값은 꺼짐입니다.

<a id="switching-back"></a>
## 되돌리기

horon-router를 멈추고 같은 설정과 인증 파일 폴더로 Go를 시작합니다. horon-router가 연결하거나 갱신한 인증 정보는 Go에서도 유효하고, 관리 키도 계속 동작합니다. 설정은 바꿀 필요가 없습니다. `routing.strategy: soonest-reset`을 쓰고 있었다면 Go는 그것을 `round-robin`으로 봅니다. Go는 `oauth.providers.codex.chatgpt-keep-alive`가 설정된 채로도 시작하고 그 값을 무시합니다. Go의 v8 관리 API로 설정을 고치면 그 키가 주석으로 옮겨갑니다. 그래서 horon-router로 돌아오면 keep-alive는 다시 꺼져 있습니다.

<a id="details"></a>
## 자세히 보기

[PARITY.md](PARITY.md)는 지금 동작하는 것을 요약하고, 모든 경로, 설정, 플래그, Go 테스트 모음을 항목별로 대조한 결과로 연결합니다.
