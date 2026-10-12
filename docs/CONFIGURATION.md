# 설정

> 이 문서에서 알 수 있는 것: `config.yaml`에 넣을 수 있는 설정과 각 설정이 하는 일.

hyper-router는 YAML 파일 하나, `config.yaml`을 읽습니다. 파일 경로는 `--config`로 넘깁니다. 넘기지 않으면 서버는 현재 디렉터리의 `config.yaml`을 읽습니다. CLIProxyAPI의 v8 구조(여기서 보여 주는 형태)와 예전의 평면 구조를 모두 받아들이고, CLIProxyAPI의 [`config.example.yaml`](https://github.com/router-for-me/CLIProxyAPI/blob/6fecc6e5567912661654a4eaf9b8f5436facd1c2/config.example.yaml)에 있는 설정도 모두 받아들입니다. 아직 이식하지 않은 기능의 설정은 파일에 남아 있어도 아무 효과가 없습니다.

서버는 이 파일을 지켜봅니다. 대부분의 변경은 재시작 없이 1초 안에 반영됩니다. `server.host`, `server.port`, `server.tls`, `server.trusted-proxies`는 시작할 때만 읽습니다. 값을 바꾼 뒤에는 재시작하십시오. 대시보드나 관리 API가 설정을 바꾸면 그 키만 다시 씁니다. 주석까지 포함한 나머지 부분은 그대로 남습니다.

<a id="a-complete-small-example"></a>
## 작은 예시 하나

```yaml
config-version: 8
server:
  host: "127.0.0.1"
  port: 8317
access:
  api-keys:
    - "your-client-key"
management:
  secret-key: "your-management-key"
oauth:
  auth-dir: "~/.hyper-router/auth"
routing:
  strategy: round-robin
  session-affinity: true
  retry:
    request-retry: 2
observability:
  logs:
    logging-to-file: true
```

<a id="server"></a>
## 서버

| 설정 | 하는 일 |
|---|---|
| `host` | 접속을 받을 주소입니다. `"127.0.0.1"`은 이 컴퓨터에서 온 요청만 받고, `""`는 모든 네트워크 인터페이스에서 받습니다. |
| `port` | 포트입니다. 기본값은 `8317`입니다. |
| `tls.enable`, `tls.cert`, `tls.key` | 같은 포트에서 이 인증서와 키 파일로 HTTPS를 제공합니다. |
| `trusted-proxies` | `X-Forwarded-For` 헤더를 믿을 역방향 프록시(사용자 대신 요청을 받아 넘겨 주는 중간 서버)나 터널의 주소입니다. 같은 컴퓨터의 터널이 요청을 넘겨 줄 때 설정합니다. 예를 들어 `[127.0.0.1, "::1"]`입니다. [안전하게 실행하기](GETTING-STARTED.md#running-it-safely)를 보십시오. |

<a id="access"></a>
## 접속

| 설정 | 하는 일 |
|---|---|
| `api-keys` | 내 도구가 `Authorization: Bearer <key>`, `x-api-key`, `x-goog-api-key`, `?key=` 형식으로 보내는 클라이언트 키입니다. 최소 하나는 설정하십시오. 목록이 비어 있으면 서버에 닿을 수 있는 사람은 누구나 이 서버를 쓸 수 있습니다. |

<a id="management"></a>
## 관리

| 설정 | 하는 일 |
|---|---|
| `secret-key` | 대시보드와 관리 API의 비밀번호입니다. 평문 값은 처음 시작할 때 bcrypt 해시로 바뀝니다. `ADMIN_PASSWORD` 환경 변수로도 설정할 수 있습니다. 이 값들과 `--password`가 모두 없으면 관리 API가 인증 없이 열립니다. |
| `allow-remote` | 기본값 `false`에서는 이 컴퓨터에서 온 관리 요청만 받습니다. |
| `disable-control-panel` | `true`이면 `/management.html`에서 대시보드를 제공하지 않습니다. 관리 API는 계속 동작합니다. |

<a id="oauth"></a>
## OAuth

| 설정 | 하는 일 |
|---|---|
| `auth-dir` | 계정 로그인 파일을 두는 폴더입니다. 기본값은 `~/.hyper-router`입니다(Go의 `~/.cli-proxy-api`와 다릅니다). `~`는 실제 경로로 확장됩니다. 서버가 이 폴더를 지켜보므로 파일을 추가하거나 고치거나 지우면 재시작 없이 반영됩니다. |
| `providers.codex.chatgpt-keep-alive` | hyper-router가 더한 설정입니다. 기본값은 `false`입니다. `true`이면 프록시마다 `chatgpt.com`으로 가는 유휴 연결을 최대 2개까지 90초 동안 유지합니다. 그래서 Codex HTTP 요청이 TCP와 TLS 핸드셰이크를 건너뜁니다. 끄면 CLIProxyAPI와 마찬가지로 Codex 요청마다 자기 연결을 새로 엽니다. 값을 바꾸면 재시작 없이 다음 요청부터 적용됩니다. Go는 파일에 이 키가 있어도 무시하고 시작합니다. Go의 v8 관리 API로 설정을 수정하면 이 키가 주석으로 옮겨집니다. 그래서 hyper-router로 돌아오면 설정이 다시 꺼진 상태가 됩니다. Go의 v0 엔드포인트로 수정하면 이 키가 그대로 남습니다. Go의 v8 API로 이 키를 설정하면 Go가 모르는 다른 키와 마찬가지로 거부됩니다. |

<a id="api-keys"></a>
## API 키

계정 로그인과 달리 제공자 API 키(프로그램이 쓰는 비밀번호)는 여기에 제공자별로 묶어 둡니다. `claude`, `codex`, `gemini`, `vertex`, `xai`, 그리고 OpenAI API를 쓰는 서비스를 위한 `openai-compatibility`입니다. 대시보드의 제공자 키 페이지에서 편집합니다. OpenAI 호환 서비스를 쓰는 예입니다.

```yaml
api-keys:
  openai-compatibility:
    - name: openrouter
      base-url: "https://openrouter.ai/api/v1"
      keys:
        - api-key: "sk-or-..."
      models:
        - name: "moonshotai/kimi-k2"
          alias: "kimi-k2"
```

<a id="routing"></a>
## 라우팅

서버가 요청마다 어느 계정을 쓸지 고르는 방식입니다. 자세한 설명은 [MULTI-ACCOUNT.md](MULTI-ACCOUNT.md)에 있습니다.

| 설정 | 하는 일 |
|---|---|
| `strategy` | `round-robin`(기본값), `fill-first`, `weighted-round-robin`, `soonest-reset` 중 하나입니다. `soonest-reset`은 실험적이며 직접 켜야 합니다. 주간 창이 가장 먼저 초기화되는 계정을 고릅니다. |
| `session-affinity` | `true`이면 대화를 첫 요청을 처리한 계정에 묶어 둡니다. 설치 스크립트는 새 설정 파일에 `true`를 씁니다. 생략하면 Go와 마찬가지로 서버 기본값은 `false`입니다. |
| `session-affinity-ttl` | 대화가 자기 계정에 묶여 있는 시간입니다. 기본값은 `"1h"`입니다. |
| `retry.request-retry` | 시도가 실패한 뒤 계정들을 다시 도는 추가 라운드 수입니다. 기본값은 `0`입니다. |
| `retry.max-retry-credentials` | 라운드마다 시도하는 계정 수의 상한입니다. `0`이면 전부 시도합니다. |
| `retry.max-retry-interval` | 재시도 전에 계정이 쿨다운(잠시 쉬게 하는 시간)에서 벗어나기를 기다리는 최대 시간(초)입니다. `0`이면 기다리지 않습니다. |
| `cooldown.disable-cooling` | `true`이면 실패한 계정을 쉬게 하지 않고 순환에 계속 둡니다. |
| `cooldown.transient-error-cooldown-seconds` | 일시적인 업스트림(요청을 실제로 받아 처리하는 제공자 서버) 오류 뒤에 계정을 쉬게 하는 시간입니다. `0`이면 60초이고, 음수면 이 기능을 끕니다. |
| `cooldown.save-cooldown-status` | `true`이면 쿨다운을 `auth-dir`의 `.cds` 파일에 남깁니다. 재시작해도 잊지 않습니다. |
| `cooldown.max-trusted-cooldown` | hyper-router가 더한 설정입니다. 계정이 사용 한도에 닿으면 제공자가 초기화 시점을 알려 줍니다. 며칠 뒤일 때도 있고, 계정은 그때까지 쉽니다. 제공자가 예고보다 일찍 초기화하는 일이 잦습니다. 그래서 hyper-router는 알려 준 시간을 최대 이 값만큼만 믿습니다(기본값 `"1h"`. Go 기간 형식인 `"90m"`이나 초를 뜻하는 숫자도 쓸 수 있습니다. 최소 10초입니다). 그 뒤에는 다음 요청을 통과시켜 확인합니다. 그래도 한도가 남아 있으면 다음 휴식은 두 배가 되고, 알려 준 초기화 시점을 넘지 않습니다. `0`이면 CLIProxyAPI처럼 알려 준 초기화 시점을 그대로 믿습니다. 읽을 수 없는 값은 로그에 남기고 기본값을 씁니다. [Go와 다른 점](DIFFERENCES-FROM-GO.md#deliberate-differences)을 보십시오. |

<a id="optional-preset-fail-over-only-on-429"></a>
### 선택 프리셋: 429일 때만 다른 계정으로 전환

주석으로 둔 이 프리셋은 Claude와 Codex OAuth(비밀번호 없이 로그인하는 방식) 계정에서 429 쿨다운과 다른 계정으로의 전환을 유지합니다. 아래에 나열한 429가 아닌 상태 코드에는 시도가 실패한 뒤의 일반적인 전환을 멈춥니다. 다만 쿨다운은 걸지 않습니다. 인증 정보 준비 실패와 켜 둔 `requests.streaming.bootstrap-retries`는 여전히 다른 시도를 일으킬 수 있습니다. 목록에 없는 상태 코드는 평소처럼 처리합니다. 이 프리셋은 OAuth 계정에만 적용됩니다. 요금제 한도와 제공자 검사는 그대로 둡니다. 인증 정보 자체의 `request_scoped_errors` 규칙이 우선합니다.

블록의 주석을 풀면 사용할 수 있습니다. `oauth-request-scoped-errors`는 `oauth.request-scoped-errors`의 예전 표기입니다. 둘 중 하나만 쓰십시오. `(?s).*`는 빈 본문까지 포함해 어떤 본문과도 일치합니다. 규칙에는 본문 일치 조건이 필요합니다. 상태 코드만으로는 일치하지 않습니다.

```yaml
# oauth-request-scoped-errors:
#   claude: &plan-limits-only
#     - {status: 429, match-regexr: ["(?s).*"], action: continue-and-cooldown}
#     - {status: 400, match-regexr: ["(?s).*"], action: stop}
#     - {status: 401, match-regexr: ["(?s).*"], action: stop}
#     - {status: 402, match-regexr: ["(?s).*"], action: stop}
#     - {status: 403, match-regexr: ["(?s).*"], action: stop}
#     - {status: 404, match-regexr: ["(?s).*"], action: stop}
#     - {status: 408, match-regexr: ["(?s).*"], action: stop}
#     - {status: 500, match-regexr: ["(?s).*"], action: stop}
#     - {status: 502, match-regexr: ["(?s).*"], action: stop}
#     - {status: 503, match-regexr: ["(?s).*"], action: stop}
#     - {status: 504, match-regexr: ["(?s).*"], action: stop}
#   codex: *plan-limits-only
```

<a id="requests"></a>
## 요청

| 설정 | 하는 일 |
|---|---|
| `proxy-url` | 모든 업스트림 요청에 쓸 외부 프록시입니다. 예를 들어 `socks5://127.0.0.1:1080`이나 `http://proxy:3128`입니다. 계정 하나만 자기 프록시를 쓸 수도 있습니다. [계정별 프록시](MULTI-ACCOUNT.md#per-account-proxies-and-request-shaping)를 보십시오. |

Claude 계정은 `HTTPS_PROXY`와 `HTTP_PROXY`를 무시합니다. 프록시를 따로 지정하지 않으면 직접 접속합니다. 프록시가 필요하면 `requests.proxy-url`이나 계정의 `proxy_url`을 설정하십시오. Anthropic이 아닌 사용자 지정 base URL을 겨냥한 Claude API 키는 표준 전송 방식을 씁니다. 이 경로는 환경 변수 프록시를 물려받을 수 있습니다.

<a id="observability"></a>
## 로그와 관측

| 설정 | 하는 일 |
|---|---|
| `logs.logging-to-file` | `true`이면 로그를 표준 출력 대신 `logs` 폴더의 `main.log`에 씁니다. 파일은 교체하며 관리합니다. |
| `logs.logs-max-total-size-mb` | 로그 파일이 쓸 수 있는 최대 디스크 공간입니다. |
| `logs.request-log` | `true`이면 요청마다 파일 하나를 만들고, 그 안에 클라이언트와 업스트림의 요청·응답 구획을 씁니다. 알려진 민감 헤더와 URL 필드는 가립니다. 그래도 본문에는 인증 정보와 사적인 프롬프트, 출력이 담길 수 있습니다. Realtime 클라이언트 시크릿도 여기에 들어갑니다. **주의:** 로그는 공개하지 마십시오. |

<a id="worker-threads"></a>
## 작업 스레드

최상위의 `worker-threads: 4`는 요청을 처리하는 스레드 수를 정합니다. 지정하지 않으면 hyper-router는 CPU 개수와 2 중 작은 값을 씁니다. 한 사람이 쓰기에는 충분합니다. 프롬프트가 300 KB인 코딩 에이전트 요청은 CPU를 약 20~30 ms 쓰고(작은 채팅 요청은 약 1 ms), 요청 시간의 대부분은 제공자를 기다리는 데 쓰입니다. 스레드가 하나 늘 때마다 해제한 메모리를 자기 풀로 따로 가질 수 있습니다. 그래서 스레드가 많을수록 상주 메모리 크기가 커집니다. `TOKIO_WORKER_THREADS` 환경 변수가 이 값을 덮어씁니다.

이 키는 시작할 때 `-config` 파일(기본값 `./config.yaml`)에서 한 번 읽습니다. 원격 저장소(`PGSTORE_*`, `OBJECTSTORE_*`, `GITSTORE_*`)나 Home이 설정을 넘겨주기 전에 읽습니다. 저장소의 `config.yaml`이나 Home이 넘긴 설정에서는 효과가 없습니다. 로컬 `-config` 파일은 그대로 적용됩니다. 그런 곳에서는 `TOKIO_WORKER_THREADS`를 설정하십시오. 그곳의 값이 실행 중인 스레드 수와 다르면 경고를 로그에 남깁니다.

이 설정은 hyper-router에만 있습니다. CLIProxyAPI는 파일에 이 키가 있어도 시작하고 v0 관리 API 저장을 거쳐도 유지합니다. 하지만 v8 설정 쓰기는 이 키를 주석으로 만들고, 이 키가 들어 있는 `config.yaml` 전체를 올리면 거부됩니다. 두 서버가 설정을 공유한다면 `TOKIO_WORKER_THREADS`를 쓰십시오. [DIFFERENCES-FROM-GO.md](DIFFERENCES-FROM-GO.md)를 보십시오.

<a id="environment"></a>
## 환경 변수

- `ADMIN_PASSWORD`: `management.secret-key` 대신 쓰는 관리 키입니다. 이것도 없고 `management.secret-key`도 없으면 관리 API가 인증 없이 열립니다.
- `TOKIO_WORKER_THREADS`: `worker-threads` 대신 쓰는 요청 스레드 수입니다.
- `RUST_LOG`: 로그 수준을 덮어씁니다. 예를 들어 `RUST_LOG=debug`입니다.
- 작업 디렉터리의 `.env` 파일은 CLIProxyAPI와 마찬가지로 시작할 때 읽습니다.
- `PGSTORE_*`, `OBJECTSTORE_*`, `GITSTORE_*`는 CLIProxyAPI와 마찬가지로 설정과 계정 파일을 PostgreSQL, S3 호환 버킷, git 저장소에 둡니다.
