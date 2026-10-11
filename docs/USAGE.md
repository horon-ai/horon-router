# 사용량과 한도 조회

> 이 문서에서 알 수 있는 것: 토큰 사용량, 계정별 사용 한도, 남은 한도를 어디서 어떻게 확인하는지와, 서버 안에서 그 정보가 어떻게 모이고 쓰이는지.

이 문서는 설정 키만 나열하지 않고, 값이 만들어지고 저장되고 쓰이는 경로를 설명합니다. 설정 키는 [설정](CONFIGURATION.md)에, 계정 선택과 쿨다운은 [여러 계정](MULTI-ACCOUNT.md)에 있습니다.

<a id="the-short-version"></a>
## 먼저 요약

제공자(Claude, Codex 같은 AI 서비스)는 "남은 토큰 수"를 알려 주지 않습니다. 알려 주는 것은 한도 창(window)별 사용 비율과 초기화 시각입니다. 창은 "5시간", "7일"처럼 기간이 정해진 사용 한도 단위입니다. hyper-router가 보여 주는 "남은 양"은 이 비율에서 나온 값입니다.

사용량 정보는 서로 다른 네 가지가 있고, 서로 다른 곳에서 만들어집니다.

| 종류 | 무엇을 알려 주는가 | 언제 갱신되는가 | 어디에 저장되는가 |
|---|---|---|---|
| 한도 관찰(passive) | 계정의 창별 사용 비율과 초기화 시각 | 요청 응답이 올 때마다 | 서버 메모리, 재시작하면 사라짐 |
| 한도 조회(live) | 제공자에게 직접 물은 사용량 | 대시보드에서 버튼을 눌렀을 때만 | 열어 둔 대시보드 탭의 메모리 |
| 사용 기록(usage record) | 요청 한 건당 토큰 수, 지연 시간, 성공 여부 | 업스트림 요청이 끝날 때마다 | 서버 메모리의 큐, 기본 60초 보관 |
| 쿨다운 기록 | 한도에 걸린 계정이 언제까지 쉬는지 | 429 같은 오류가 올 때 | 메모리, `save-cooldown-status`를 켜면 `.cds` 파일 |

**주의:** 어느 것도 "하루 동안 몇 토큰을 썼는지"를 서버가 합산해서 보관하지 않습니다. 누적 통계가 필요하면 사용 기록을 밖으로 받아서 직접 모아야 합니다.

<a id="passive-observation"></a>
## 한도 관찰: 요청에 실려 오는 헤더 읽기

요청을 보낼 때마다 업스트림(요청을 실제로 받아 처리하는 제공자 서버)의 응답 헤더에서 한도 정보를 골라 계정별로 저장합니다. 추가 요청은 보내지 않으므로 비용이 없습니다.

<a id="what-it-reads"></a>
### 무엇을 읽는가

| 제공자 | 읽는 헤더 |
|---|---|
| Claude | `retry-after`와 `anthropic-ratelimit-unified-`로 시작하는 모든 헤더(5시간 창 `5h`, 7일 창 `7d`, 초과 사용 `overage` 등) |
| Codex | `retry-after`, `x-ratelimit-`로 시작하는 헤더, `x-codex-` 헤더 중 한도 표지(`-allowed`, `-limit-reached`, `-used-percent`, `-window-minutes`, `-reset-after-seconds`, `-reset-at`)가 붙은 것, `x-codex-plan-type`, `x-codex-active-limit`, `x-codex-credits-` |
| Devin | 상태 조회(status refresh) 응답에서 만든 한도 값 |

Codex는 WebSocket으로 연결했을 때 `codex.rate_limits` 이벤트와 오류 프레임에 한도 정보가 들어옵니다. 서버는 이것도 같은 `X-Codex-*` 헤더 모양으로 바꿔서 저장합니다.

헤더 값은 다음 규칙으로 걸러냅니다.

- 값이 비어 있거나 512바이트를 넘거나 제어 문자가 들어 있으면 버립니다.
- 같은 이름이 여러 번 오면 마지막 값을 씁니다.
- 한 스냅샷에 최대 64개만 남깁니다. 우선순위가 높은 것부터 남기며, 순서는 `retry-after`, 요금제와 크레딧, 5시간·주간 창, 코드 리뷰 한도, 그 밖의 `x-codex-`입니다.

Claude 응답은 `retry-after`와 `anthropic-ratelimit-unified-` 헤더만 남기고 모두 이름 순으로 정렬합니다. 구현은 `crates/hr-exec/src/quota.rs`의 `claude_signals`와 `crates/hr-exec/src/codex_quota.rs`의 `collect_signals`에 있습니다.

<a id="how-it-is-stored"></a>
### 어떻게 저장하는가

계정마다 스냅샷을 두 종류 가집니다. 하나는 계정 전체의 최신 스냅샷이고, 하나는 모델별 최신 스냅샷입니다(`crates/hr-exec/src/codex_quota.rs`의 `QuotaSignals`).

- 새 응답에 한도 신호가 있으면 이전 스냅샷을 **통째로 덮어씁니다**. 이전 값과 합치지 않습니다.
- 한도 신호가 없는 응답은 스냅샷을 건드리지 않습니다.
- 스냅샷은 서버 메모리에만 있습니다. 재시작하면 사라지고, 계정으로 첫 요청이 성공하면 다시 채워집니다.
- 한 번이라도 쓴 계정마다 항목이 하나 남습니다. 계정을 지워도 서버를 재시작하기 전에는 항목이 남을 수 있습니다.

관리 API가 계정 목록을 돌려줄 때 이 값이 `quota`(`observed_at`과 `signals`)와 `model_quotas`로 들어갑니다(`crates/hr-server/src/management/auth_files.rs`). 대시보드의 Credentials, Quotas, Overview 페이지가 이 값을 읽어 그립니다.

<a id="where-it-is-used"></a>
### 어디에 쓰는가

한도 관찰은 화면에 보여 주는 데서 끝나지 않고 계정 선택에도 쓰입니다(`crates/hr-server/src/scheduler.rs`의 `Windows::observed`).

1. 관찰한 헤더에서 "긴 창(주간)"과 "짧은 창(5시간)"을 읽습니다. Claude는 `7d`와 `5h`를 쓰고, Codex는 창 길이가 더 긴 쪽을 주간으로 봅니다.
2. 창이 소진됐는지 판단합니다. Claude는 상태가 `rejected`이거나 사용 비율(`utilization`)이 1.0 이상이면 소진으로 봅니다. Codex는 `used-percent`가 100 이상이면 소진으로 봅니다. `x-codex-limit-reached: true`도 소진 표시입니다.
3. 소진된 창이 언제까지 이어지는지 계산합니다. 초기화 시각이 있으면 그 시각까지, 없으면 창 길이만큼, 그것도 없으면 5시간으로 가정합니다.
4. 계정이 소진 상태이면 라우팅에서 뒤로 밀립니다. `soonest-reset` 방식은 주간 창이 가장 먼저 초기화되는 계정을 앞세웁니다. 초기화 시각을 모르는 계정에는 요청을 한 번 보내 시각을 알아냅니다.

**주의:** 이 판단은 마지막으로 관찰한 시점의 값에 기댑니다. 한동안 요청을 받지 않은 계정은 값이 오래됐을 수 있으므로, 서버는 초기화 시각이 지난 창을 "한 바퀴 돌았다"고 보고 다시 시험해 봅니다.

<a id="live-check"></a>
## 한도 조회: 제공자에게 직접 묻기

대시보드의 Quotas 페이지에서 Check quota를 누르면(Overview에서는 Check limits) 서버가 제공자의 사용량 주소를 그 계정의 토큰으로 호출합니다. 버튼을 누르기 전에는 호출하지 않습니다(`ui/src/quota.ts`).

<a id="addresses-called"></a>
### 호출하는 주소

| 제공자 | 주소 |
|---|---|
| Claude | `https://api.anthropic.com/api/oauth/usage` |
| Codex | `https://chatgpt.com/backend-api/wham/usage` |
| Kimi | `https://api.kimi.com/coding/v1/usages` |
| Kimi(ai) | `https://api.kimi.ai/coding/v1/usages` |
| Command Code | `POST /quota/fetch` → `/alpha/whoami`, `/alpha/billing/credits`, `/alpha/billing/subscriptions`, `/alpha/usage/summary?since=` |

<a id="call-path"></a>
### 호출 경로

1. 대시보드가 `POST /requests/api-call`을 보냅니다. 요청에 인증 헤더 값으로 `Bearer $TOKEN$`을 적고 계정의 `authIndex`를 함께 넘깁니다.
2. 서버가 `$TOKEN$`을 그 계정의 실제 토큰으로 바꿉니다. 바뀐 토큰은 제공자에게만 가고 대시보드로 돌아오지 않습니다. 요청은 그 계정에 설정한 프록시를 거칩니다(`crates/hr-server/src/management/api_call.rs`).
3. 제한은 60초 시간 제한과 응답 본문 64 MiB입니다.
4. Claude에는 `anthropic-beta: oauth-2025-04-20`과 Claude CLI의 `User-Agent`를, Codex에는 `codex-tui`의 `User-Agent`와 `Chatgpt-Account-Id`를 붙입니다.
5. 대시보드가 응답을 창 목록으로 바꿔 그립니다. Command Code는 서버가 모은 본문을 쓰며, 5시간 창과 주간 창에 더해 결제 기간 전체 한도("Period limit" / "전체 한도")와 월간·구매·무료 크레딧 합계("Credits left")를 보여 줍니다. `quota/fetch`가 없는 서버에서는 예전처럼 credits 주소만 호출합니다. Claude는 Claude Code의 `/usage`와 같은 행(5시간, 주간, 모델별 주간, 초과 사용)을 보여 줍니다. 초과 사용 금액은 센트 단위 값을 달러로 바꿔 표시합니다.

응답이 401 또는 403이면 "제공자가 계정 토큰을 거부했다"는 안내를 보여 줍니다. 이때는 토큰을 갱신하거나 계정을 다시 연결합니다.

<a id="when-a-provider-has-no-usage-address"></a>
### 제공자에 사용량 주소가 없을 때

- 그 제공자를 위한 사용 한도 플러그인(`quota_provider`)이 있으면 `POST /plugins/{id}/quota`로 플러그인에게 묻습니다.
- 플러그인이 없으면 계정 파일의 선언형 `quota_probe`를 씁니다. 계정 파일에 URL, 메서드, 헤더, 응답에서 값을 꺼낼 경로를 적어 두면 서버가 그대로 호출해서 결과를 한도 모양으로 바꿉니다(`crates/hr-server/src/management/quota_probe.rs`).
- 둘 다 없으면 대시보드는 "이 제공자의 한도 출처가 없다"고 표시합니다. 이 경우에도 한도 관찰 값이 있으면 그 값이 보입니다.

<a id="what-the-screen-shows"></a>
### 화면에 무엇이 보이는가

화면은 같은 계정에 대해 **이번 탭에서 조회한 값이 있으면 그것을, 없으면 한도 관찰 값을** 보여 줍니다(`ui/src/quota.ts`의 `limits`). 조회 결과는 열어 둔 탭의 메모리에만 있으므로 새로고침하면 사라집니다. 서버는 조회 결과를 저장하지 않습니다.

<a id="usage-records"></a>
## 사용 기록: 요청 한 건당 토큰 수

업스트림 요청이 하나 끝날 때마다 서버가 사용 기록(JSON) 한 줄을 만듭니다. 설정에서 사용 통계를 켠 경우에만 만듭니다.

```yaml
observability:
  usage:
    usage-statistics-enabled: true
    redis-usage-queue-retention-seconds: 60
```

- `usage-statistics-enabled`: 기본값은 꺼짐입니다. 꺼져 있으면 기록을 만들지 않습니다. 관리 API가 꺼져 있을 때도 기록하지 않습니다.
- `redis-usage-queue-retention-seconds`: 큐에 기록을 두는 시간(초)입니다. 값이 없거나 0 이하이면 60초, 3600을 넘으면 3600으로 맞춥니다(`crates/hr-server/src/usage.rs`).

<a id="what-a-record-holds"></a>
### 기록에 들어 있는 것

- `tokens`: `input_tokens`, `output_tokens`, `reasoning_tokens`, `cached_tokens`, `cache_read_tokens`, `cache_creation_tokens`, `total_tokens`.
- `token_breakdown`: 같은 토큰 수를 입력(캐시 안 된 부분, 캐시에서 읽은 부분, 캐시에 쓴 부분)과 출력(추론이 아닌 부분, 추론 부분)으로 나눈 값과 `quality`.
- 그 밖에 `timestamp`, `latency_ms`, `ttft_ms`(첫 토큰까지 걸린 시간), `provider`, `model`, `alias`, `auth_index`, `source`, `endpoint`, `stream`, `failed`, `fail`(상태 코드와 본문), `request_id`, `session_id`가 있습니다.
- `api_key`, `client_ip`, `user_agent`, 응답 헤더도 들어갑니다.

**주의:** 사용 기록에는 클라이언트 키와 IP가 들어 있습니다. 큐를 읽는 도구는 관리 키를 가진 사람만 쓰게 하고, 기록을 밖으로 저장할 때는 비공개로 보관합니다.

<a id="how-tokens-are-counted"></a>
### 토큰 수를 세는 방식

제공자마다 `usage`의 모양이 다르므로, 서버는 제공자별 규칙으로 읽어서 같은 모양으로 맞춥니다(`crates/hr-server/src/usage_record.rs`).

- **포함형(Subset):** 캐시 토큰이 입력 토큰 안에 들어 있고, 추론 토큰이 출력 토큰 안에 들어 있는 제공자입니다. 캐시가 입력보다 크거나 추론이 출력보다 크면 모순으로 봅니다.
- **독립형(Independent):** 입력, 캐시 읽기, 캐시 쓰기가 따로 세어지는 제공자입니다. 입력 합계는 세 값을 더해서 만듭니다.
- **추론 분리형:** 출력과 추론이 따로 세어지는 제공자입니다.

`token_breakdown.quality`는 세 가지입니다.

| 값 | 뜻 |
|---|---|
| `complete` | 합계와 항목이 맞아떨어집니다. |
| `unclassified` | 제공자가 알려 준 합계가 항목의 합보다 큽니다. 남는 만큼은 `unclassified_tokens`에 들어 있습니다. |
| `inconsistent` | 수치가 모순입니다(예: 음수, 캐시가 입력보다 큼). 항목은 비우고 합계만 `unclassified_tokens`에 넣습니다. |

<a id="how-to-read-records"></a>
### 기록 읽는 방법

두 가지 방법이 있습니다. 같은 큐를 함께 쓰므로 한쪽이 읽으면 다른 쪽은 그 기록을 못 봅니다.

1. **관리 API로 꺼내 가기:** `GET /observability/usage/queue?count=N`이 가장 오래된 기록 N건을 꺼내 돌려줍니다. `count`를 생략하면 1건이고, 양의 정수가 아니면 400을 돌려줍니다. 꺼낸 기록은 큐에서 사라집니다. 대시보드의 Usage 페이지 실시간 보기가 이 방법을 씁니다. 그래서 켜기 전에 다른 수집기가 같은 큐를 읽고 있지 않은지 확인하는 안내가 나옵니다.
2. **Redis 프로토콜로 구독하기:** 같은 포트에서 Redis 프로토콜(RESP) 연결을 받습니다. 관리 키로 `AUTH`한 뒤 `LPOP`/`RPOP`으로 꺼내거나, `usage`와 `errors` 채널을 `SUBSCRIBE`합니다. 구독자가 한 명이라도 연결되어 있으면 기록은 큐에 쌓이지 않고 구독자에게만 전달됩니다. 구독자 버퍼는 256건이고, 가득 차면 그 구독이 끊깁니다(`crates/hr-server/src/resp.rs`). Home 모드에서는 이 연결을 거부합니다.

기록은 메모리에만 있습니다. 보관 시간이 지난 기록은 큐에 접근할 때마다 지워지고, 관리 API를 끄면 큐가 비워집니다. 장기 보관과 합산은 서버 밖에서 하며, 구독 도구로 받아서 직접 쌓아야 합니다.

<a id="request-counts"></a>
### 요청 수 통계

- `GET /observability/usage/api-keys`는 API 키 항목별(제공자, 주소, 키) 성공 수와 실패 수, 최근 요청 구간별 건수를 돌려줍니다. 대시보드의 Providers 페이지가 15초마다 읽어 키별 트래픽을 보여 줍니다.
- 이 숫자는 토큰이 아니라 요청 건수입니다.

<a id="when-a-limit-is-hit"></a>
## 한도에 걸렸을 때

업스트림이 429를 돌려주면 서버가 오류의 범위를 정하고 계정을 쉬게 합니다(`crates/hr-exec/src/quota.rs`의 `classify`).

| 상황 | 쉬는 범위 |
|---|---|
| Claude의 5시간 창이나 7일 창 전체가 거부됨 | 그 계정 전체(모든 모델). `oauth.providers.claude.model-level-cooling: true`이면 그 모델만 |
| 모델 한도 같은 일반 429 | 그 계정의 그 모델만 |
| "fast" 요청의 사용 크레딧이 부족하다는 오류 | 그 요청만 실패하고 계정과 모델 상태는 그대로 |

`oauth.providers.claude.model-level-cooling`과 `oauth.providers.codex.model-level-cooling`은 기본값이 `false`입니다. `true`로 바꾸면 Claude의 창 전체 거부와 Codex의 `usage_limit_reached`도 요청한 모델에만 쿨다운을 겁니다.

다시 시도할 시점은 `Retry-After` 헤더나 `anthropic-ratelimit-unified-*-reset` 같은 초기화 시각에서 읽습니다. 서버는 이 시각을 `routing.cooldown.max-trusted-cooldown`(기본 1시간)만큼만 믿고, 그 뒤에 요청을 한 번 보내 확인합니다. 쿨다운 기록에는 `observed_at`도 함께 남습니다(`crates/hr-server/src/cooldown_store.rs`). `routing.cooldown.save-cooldown-status`를 켜면 쿨다운이 `auth-dir`의 `.cds` 파일에 저장되어 재시작해도 이어집니다. 자세한 규칙은 [여러 계정](MULTI-ACCOUNT.md#cooldowns-and-limits)에 있습니다.

Credentials와 Quotas 페이지의 Reset cooldown은 서버가 가진 쿨다운 기록만 지웁니다. 제공자의 한도가 되돌아오지는 않습니다.

<a id="counting-tokens"></a>
## 요청의 토큰 수 세기

"남은 한도"와 다른 기능으로, 요청 본문이 몇 토큰인지 세는 기능이 있습니다. `POST /v1/messages/count_tokens`가 그 경로입니다.

- Claude의 정식 API 키 계정은 요청을 Anthropic의 `/v1/messages/count_tokens`로 전달합니다. Kimi도 업스트림으로 전달합니다.
- Claude OAuth 계정이나 정식 주소가 아닌 `base-url`을 쓰는 Claude 키는 서버 안에서 tiktoken 방식으로 직접 셉니다(`crates/hr-exec/src/claude.rs`의 `count_tokens`). Codex, Meta, xAI, OpenAI 호환 제공자의 계산과, 비 Claude 업스트림을 쓸 때의 Claude `message_start` 추정치도 같은 방식입니다. 모델에 따라 `o200k_base`나 `cl100k_base`를 고릅니다(`crates/hr-exec/src/tokenizer.rs`).
- 인코더는 처음 필요할 때 한 번 만들어 프로세스 안에서 공유합니다. 만드는 데 `o200k_base`는 약 0.15초, `cl100k_base`는 약 0.06초가 걸리고, 메모리(RSS)는 각각 약 47 MB와 24 MB를 씁니다.
- 업스트림에 토큰 세기 주소가 없어서 생긴 404는 계정과 모델의 상태를 바꾸지 않습니다. 쿨다운도 걸리지 않습니다.

<a id="what-is-not-there"></a>
## 없는 것

- **절대 토큰 잔량:** 제공자가 알려 주지 않으므로 "몇 토큰 남음"은 없습니다. 비율과 초기화 시각만 있습니다.
- **누적 합산:** 서버는 일별, 월별 토큰 합계를 저장하지 않습니다. 사용 기록은 60초(최대 1시간) 보관하는 메모리 큐입니다.
- **디스크 저장:** 한도 관찰, 사용 기록, 조회 결과는 모두 메모리에만 있습니다. 디스크에 남는 것은 `save-cooldown-status`를 켠 경우의 쿨다운 기록과 요청 로그 파일뿐입니다.
- **사용 비율로 나누는 라우팅:** 남은 비율에 비례해 요청을 나누지는 않습니다. 비율은 소진된 계정을 건너뛰고 `soonest-reset` 순서를 정하는 데만 쓰입니다. 비중을 정하려면 `weighted-round-robin`의 `weight`를 직접 정합니다.
- **예측:** 지금 속도로 쓰면 언제 한도에 닿는지 계산하지 않습니다.
- **한도 소진 시 모델 교체:** CLIProxyAPI의 `quota-exceeded.switch-project`와 `switch-preview-model`은 아직 구현되지 않았습니다. 설정 키를 파일에 둘 수는 있지만 효과가 없습니다. 현황은 [호환성 현황](PARITY.md)에 있습니다.
