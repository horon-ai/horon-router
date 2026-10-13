# 여러 계정

> 이 문서에서 알 수 있는 것: 계정 여러 개를 돌려 쓰는 방법(라우팅 방식, 세션 고정, 쿨다운, 사용 한도 보기)과 다른 컴퓨터에서 프록시로 접속하는 방법.

이 문서는 내 컴퓨터에서 내 계정과 API 키(프로그램이 쓰는 비밀번호)를 넘나드는 라우팅(요청을 어느 계정으로 보낼지 고르는 일)을 다룹니다. 다른 사람의 구독을 모아 쓰지 마십시오. [계정과 제공자 이용약관](../README.md#accounts-and-provider-terms)을 읽으십시오. 프록시(요청을 대신 전달해 주는 중간 서버)가 이미 실행 중이라고 가정합니다([GETTING-STARTED.md](GETTING-STARTED.md)).

<a id="add-the-accounts"></a>
## 계정 추가하기

계정마다 한 번씩 로그인합니다. 대시보드에서 계정 연결을 고르거나 명령줄에서 `--claude-login`, `--codex-login`을 씁니다. 로그인 하나가 `auth-dir`의 파일 하나가 됩니다. 파일 하나가 순환에 참여하는 계정 하나입니다.

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="img/credentials-dark.png">
  <img src="img/credentials-light.png" alt="인증 정보 페이지: Claude 계정 세 개와 Codex 계정 두 개가 각각의 상태와 최근 요청과 함께 나열된 모습" width="880">
</picture>

제공자의 로그인 페이지는 브라우저가 이미 로그인해 둔 계정을 그대로 씁니다. 두 번째 Claude 계정을 추가하려면 먼저 claude.ai에서 로그아웃하십시오. 또는 시크릿 창이나 다른 브라우저 프로필에서 로그인 링크를 엽니다. `--no-browser`는 링크를 열지 않고 출력합니다. 대시보드의 인증 정보 페이지는 계정마다 상태와 함께 나열합니다. 계정을 지우지 않고 비활성화할 수도 있습니다.

<a id="routing-strategies"></a>
## 라우팅 방식

`routing.strategy`는 다음 요청을 어느 계정이 처리할지 정합니다.

| 방식 | 하는 일 | 쓰는 경우 |
|---|---|---|
| `round-robin`(기본값) | 계정을 차례로 돌며 요청을 하나씩 맡깁니다. | 계정들이 비슷하고 부하를 고르게 나누고 싶을 때입니다. |
| `fill-first` | 한 계정을 쿨다운(잠시 쉬게 하는 시간)에 들거나 한도에 닿을 때까지 쓰고, 그다음 계정으로 넘어갑니다. | 한 계정을 주로 쓰고 다른 계정은 예비로 두고 싶을 때입니다. |
| `weighted-round-robin` | 계정의 `weight`에 비례해 차례로 돕니다. 가중치가 0인 계정은 쓰지 않습니다. | 요금제 규모가 서로 다를 때입니다. |
| `soonest-reset`(실험적, horon-router가 더한 기능) | 주간 창이 가장 먼저 초기화되는 계정을 고릅니다. 그 계정이 쿨다운에 들거나 한도에 닿을 때까지 씁니다. 초기화 시점을 모르는 계정은 알아내려고 요청 하나를 받습니다. | 초기화 시각 순으로 고르고 싶을 때입니다. CLIProxyAPI는 이 값을 `round-robin`으로 읽습니다. |

`soonest-reset`은 실험적이며 직접 고를 때만 동작합니다. 이상해 보이는 점이 있으면 알려 주십시오.

`soonest-reset`은 계정을 마지막 응답의 사용량 헤더로 줄 세웁니다. 알려 준 초기화 뒤 한 시간 휴식이 끝난 계정도([쿨다운과 한도](#cooldowns-and-limits) 참고) 창을 다 쓴 것으로 보고합니다. 그래서 맨 뒤로 밀리고, 다른 계정이 다 소진되거나 쿨다운에 들었을 때만 검사합니다.

대시보드의 설정 페이지나 `config.yaml`에서 방식을 바꿀 수 있습니다.

계정마다 `priority`(기본값 0)를 둘 수도 있습니다. 프록시는 준비된 계정 중 우선순위가 가장 높은 계정만 씁니다. 그 계정들이 모두 쿨다운에 들거나 비활성화되면 낮은 우선순위로 내려갑니다. 계정별 Priority와 Weight는 대시보드의 인증 정보 페이지에서 설정하거나, 계정 파일의 최상위 `"priority"`와 `"weight"` 필드로 설정합니다.

<a id="session-affinity"></a>
## 세션 고정

설치 스크립트는 새 설정 파일에 고정을 켜 둡니다. 이미 있는 설정 파일은 건드리지 않습니다. 직접 켜려면 다음과 같이 합니다.

```yaml
routing:
  session-affinity: true
```

고정을 켜면 대화가 첫 요청을 처리한 계정에 남습니다. 제공자는 계정마다 프롬프트 캐시를 둡니다. 그래서 같은 계정에서 이어지는 다음 턴은 길게 공유하는 앞부분을 캐시에서 읽습니다. 한도를 덜 쓰고 답도 빨리 시작합니다. 대화를 다른 계정으로 옮기면 그 캐시를 버립니다. 프록시는 Claude Code, Codex, OpenCode 같은 도구가 보내는 세션 헤더로 대화를 알아봅니다. 요청에 그런 헤더가 없으면 같은 클라이언트 키와 모델로 본 대화들과 메시지를 대조합니다. 이어지는 대화는 자기 계정을 지킵니다. 앞선 대화의 갈래는 그 대화의 계정에 남습니다. 앞부분이 요약(압축)된 대화는 남긴 턴으로 알아봅니다. 앞선 대화와 맞지 않는 요청은 새 대화를 시작합니다. 그다음 턴들은 이 새 대화와 대조됩니다. 메시지 대조에는 클라이언트 키(`access.api-keys`)와 시스템 메시지가 아닌 메시지가 최소 하나 필요합니다. 둘 중 하나라도 없는 요청은 지시문과 첫 사용자 메시지로 식별합니다. 사용자 메시지도 없으면 처음 메시지들의 해시로 식별합니다.

`session-affinity`를 생략하면 horon-router와 CLIProxyAPI 모두 기본값이 꺼짐입니다. 고정은 라우팅 방식과 함께 동작합니다. 묶인 대화는 자기 계정을 지키고, 새 대화는 방식을 따릅니다. 묶인 계정이 쿨다운에 들거나 비활성화되면 대화는 다른 계정으로 옮겨집니다. `session-affinity-ttl`(기본값 `"1h"`)은 노는 대화가 자기 계정을 유지하는 시간을 정합니다. `session-affinity-subagents`(기본값 `true`)는 하위 세션도 부모의 계정에 묶습니다.

<a id="cooldowns-and-limits"></a>
## 쿨다운과 한도

제공자가 429(요청이 너무 많거나 한도를 다 씀)로 답하면, 프록시는 그 계정을 그 모델에 한해 쉬게 합니다. 그리고 요청을 다음 계정으로 보냅니다.

- 제공자가 다시 시도할 시점을 알려 주면(`Retry-After` 헤더나 알려 준 초기화 시점), 계정은 그만큼 쉽니다. 최소 10초이고, 처음에는 최대 한 시간입니다([`max-trusted-cooldown`](CONFIGURATION.md#routing)). 그다음 요청이 계정을 확인합니다. 확인이 나가는 동안 다른 요청은 다른 곳으로 갑니다. 그래도 한도가 남아 있으면 다음 휴식은 두 배가 됩니다. 알려 준 초기화 시점을 넘지는 않습니다. 대시보드는 그런 계정을 알려 준 초기화 시점까지 "다음 초기화까지 사용 제한"으로 표시하고, 다음 확인 시각도 함께 보여 줍니다.
- 그런 정보가 없으면 휴식은 1초에서 시작합니다. 429가 이어질 때마다 두 배가 되고, 최대 30분입니다.
- 계정 전체가 사용 한도를 다 썼다는 오류는 그 계정을 모든 모델에 한해 쉬게 합니다.
- 토큰(로그인 증명서 같은 값)이 거부되면(401 또는 403) 계정을 30분 동안 쉬게 합니다. 로그인이 만료됐거나 취소됐으면 대시보드에 "다시 로그인"으로 표시합니다.
- 계정에서 요청이 성공하면 그 모델의 쿨다운이 풀립니다.

`routing.retry.request-retry`(기본값 0)는 한 라운드의 시도가 모두 실패했을 때 계정들을 다시 도는 라운드를 더합니다. `routing.retry.max-retry-interval`(초, 기본값 0)은 곧바로 실패하지 않고 계정이 쿨다운에서 벗어나기를 기다리게 합니다. 인증 정보와 사용 한도 페이지의 쿨다운 초기화 버튼은 프록시가 가진 기록만 지웁니다. 제공자의 한도를 되돌려 주지는 않습니다.

선택 사항인 [429일 때만 다른 계정으로 전환하는 프리셋](CONFIGURATION.md#optional-preset-fail-over-only-on-429)은 OAuth(비밀번호 없이 로그인하는 방식) 계정에서 429일 때의 다른 계정 전환을 유지합니다. 목록에 있는 429가 아닌 상태 코드에는 시도가 실패한 뒤의 일반적인 전환을 멈춥니다. 인증 정보 준비 실패와 켜 둔 `requests.streaming.bootstrap-retries`는 여전히 다른 시도를 일으킬 수 있습니다.

<a id="read-the-quota-view"></a>
## 사용 한도 보기

사용 한도 페이지는 사용 한도 확인(또는 개요 화면의 한도 확인)을 누르면 계정마다 제공자에게 사용량을 물어봅니다. Claude 계정이면 Claude Code의 `/usage`가 보여 주는 행을 그대로 보여 줍니다.

- 현재 세션: 5시간 창입니다.
- 이번 주(모든 모델): 주간 한도입니다.
- 이번 주(Fable만), (Opus만) 등: 요금제에 모델 하나만의 주간 상한이 있으면 그것을 보여 줍니다.
- 추가 사용량: 요금제를 넘겨 쓴 유료 사용량을 달러로 보여 줍니다. 켜져 있을 때만 나옵니다.

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="img/quotas-dark.png">
  <img src="img/quotas-light.png" alt="사용 한도 페이지: 모델별 주간 상한을 포함한 Claude 세션·주간 한도와 Codex 5시간·주간 한도" width="880">
</picture>

각 행은 얼마나 썼는지와 언제 초기화되는지 보여 줍니다. Codex 계정은 5시간 창과 주간 창을 보여 줍니다. 검사 사이에도 프록시는 Codex가 응답마다 보내는 한도 헤더를 읽습니다. 그래서 개요 화면은 검사 없이도 Codex 한도를 보여 줄 수 있습니다.

<a id="per-account-proxies-and-request-shaping"></a>
## 계정별 프록시와 요청 형태

이 설정은 `auth-dir`의 계정 파일에 넣습니다. 서버는 파일이 바뀌면 다시 읽습니다.

```json
{
  "proxy_url": "socks5://user:pass@proxy.example.com:1080"
}
```

`proxy_url`은 그 계정의 요청을 자기 프록시(`http://`, `https://`, `socks5://`, `socks5h://`)로 보냅니다. `"direct"`는 모든 프록시를 건너뜁니다. 지정하지 않으면 계정은 `config.yaml`의 `requests.proxy-url`을 씁니다. 둘 다 없으면 대부분의 제공자는 `HTTPS_PROXY` 환경 변수를 따릅니다. Claude 계정은 직접 접속합니다. `config.yaml`에 있는 API 키라면 같은 설정이 그 키 항목의 `proxy-url`입니다.

Claude 요청 형태는 CLIProxyAPI의 규칙을 따릅니다. 기본 `auto` 모드는 비네이티브 클라이언트를 위해 시스템 프롬프트를 다시 쓸 수 있습니다. 끄려면 계정 파일에 `"cloak_mode": "never"`를 넣거나 `config.yaml`에 `oauth.providers.claude.disable-claude-cloak-mode: true`를 넣으십시오. 이 재작성이 다른 도구에서 Claude 구독을 써도 된다는 뜻은 아닙니다. 그런 곳에서는 API 키나 지원되는 클라우드 제공자를 쓰십시오.

<a id="codex-over-websocket"></a>
## WebSocket을 쓰는 Codex

Codex 계정은 턴마다 HTTPS 요청을 하나씩 보내는 대신, Codex CLI가 쓰는 전송 방식인 WebSocket으로 OpenAI와 통신할 수 있습니다. 계정 파일의 최상위 필드로 계정마다 켭니다.

```json
{
  "websockets": true
}
```

Codex API 키라면 `api-keys.codex` 아래 그 키 항목에 `websockets: true`를 설정합니다.

<a id="reach-the-proxy-from-other-machines"></a>
## 다른 컴퓨터에서 프록시 접속하기

가장 간단하고 안전한 방법은 [Tailscale](https://tailscale.com/)입니다. 여러 컴퓨터를 사설망에 묶어 줍니다.

1. 프록시를 돌리는 컴퓨터와 프록시를 쓰는 컴퓨터에 Tailscale을 설치합니다.
2. `config.yaml`에서 `server.host`를 프록시 컴퓨터의 Tailscale 주소로 설정하고 재시작합니다. `tailscale ip -4`가 알려 주는 `100.x.y.z` 주소입니다. 그래야 내 tailnet에서만 접속할 수 있습니다. `access.api-keys`는 그대로 설정해 두십시오.
3. 다른 컴퓨터에서는 `http://<machine-name>:8317`(또는 `100.x.y.z` 주소)을 base URL로 씁니다.

`tailscale serve`로 HTTPS를 붙이면 이 기능은 `127.0.0.1`에서 요청을 넘겨 줍니다. 그래서 `server.trusted-proxies: [127.0.0.1, "::1"]`도 설정하십시오. 다른 컴퓨터에서 대시보드를 열려면 `management.allow-remote: true`를 설정합니다. **주의:** `access.api-keys`가 없으면 Tailscale Funnel이든 다른 터널이든 프록시를 인터넷에 노출하지 마십시오. 클라이언트 키가 없으면 주소를 찾아낸 사람은 누구나 내 계정을 쓸 수 있습니다.

<a id="a-worked-example-three-claude-and-two-codex-accounts"></a>
## 예제: Claude 계정 세 개와 Codex 계정 두 개

이 예제는 한 사람이 자기 컴퓨터에서 자기 계정을 쓸 때를 위한 것입니다. Claude Max 계정이 요청을 먼저 받습니다. 그 계정이 쿨다운에 드는 동안 Claude Pro 계정 두 개가 대신 맡습니다. 대화는 계정이 준비된 동안 그 계정에 남습니다. Codex 계정 두 개는 모두 WebSocket을 씁니다. Claude 구독은 [계정과 제공자 이용약관](../README.md#accounts-and-provider-terms)에 나온 대로 네이티브 Anthropic 애플리케이션에서만 쓰십시오.

`config.yaml`:

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
  auth-dir: "~/.horon-router/auth"
routing:
  strategy: fill-first
  session-affinity: true
  session-affinity-ttl: "1h"
  retry:
    request-retry: 1
    max-retry-interval: 30
```

다섯 번 로그인합니다(Claude 계정 세 개, Codex 계정 두 개). 그다음 대시보드의 인증 정보 페이지에서 아래 필드를 설정하거나 `~/.horon-router/auth`의 파일에 더합니다.

| 계정 파일 | 필드 | 이유 |
|---|---|---|
| Claude Max | `"priority": 10` | 먼저 씁니다. |
| Claude Pro, 첫 번째 | `"priority": 0` | Max 계정이 쿨다운에 들 때 씁니다. |
| Claude Pro, 두 번째 | `"priority": 0` | Max 계정이 쿨다운에 들 때 씁니다. |
| Codex, 첫 번째 | `"websockets": true` | WebSocket 전송을 씁니다. |
| Codex, 두 번째 | `"websockets": true` | WebSocket 전송을 씁니다. |

`fill-first`에서는 제공자마다 우선순위가 가장 높은 계정이 쿨다운에 들 때까지 요청을 처리합니다. 그다음 준비된 계정이 이어받습니다. 묶인 대화는 그 계정이 준비된 동안 자기 계정에 남습니다. `fill-first`를 `soonest-reset`으로 바꾸면 초기화 시각 순으로 계정을 고릅니다. `weight` 필드와 함께 `weighted-round-robin`으로 바꾸면 요금제 규모에 따라 요청을 나눕니다.
