# 캐시 처리

> 이 문서에서 알 수 있는 것: horon-router가 캐시를 어떻게 다루는지. 제공자의 프롬프트 캐시를 살리는 방법, 서버 안에 두는 캐시의 종류와 한계, 캐시하지 않는 것.

<a id="the-short-version"></a>
## 먼저 요약

horon-router는 AI 응답을 저장해 두었다가 다시 내주지 않습니다. 같은 요청이 반복되면 매번 업스트림(요청을 실제로 받아 처리하는 제공자 서버)에 보냅니다.

캐시와 관련된 일은 세 가지입니다.

1. **제공자의 프롬프트 캐시를 살립니다.** 프롬프트 캐시는 제공자가 대화 앞부분을 기억해 두었다가, 다음 요청에서 그 부분을 싸고 빠르게 처리해 주는 기능입니다. 프록시(요청을 대신 전달해 주는 중간 서버)는 이 기능이 잘 작동하도록 요청을 맞추고 대화를 같은 계정에 묶어 둡니다. 예외로 Command Code 계정은 요청에 캐시 표시를 넣지 않고 세션 ID만 보냅니다([Command Code 절](#command-code)).
2. **서버 안에 상태 캐시를 둡니다.** 세션과 계정의 연결, 사고(thinking) 블록 복원 같은 정확성을 위한 기억입니다. 모두 메모리에 있고 크기와 보관 시간에 한도가 있습니다.
3. **연결을 다시 씁니다.** HTTP 연결과 TLS(암호화 연결) 세션을 재사용해서 접속 시간을 줄입니다.

| 구분 | 캐시하는가 | 어디에 | 얼마나 |
|---|---|---|---|
| AI 응답 | 하지 않습니다 | 없음 | 없음 |
| 프롬프트 캐시 | 제공자가 합니다. 프록시는 요청을 맞추고 계정을 고정합니다 | 제공자 쪽 | 제공자가 정합니다 |
| 세션과 계정의 연결 | 합니다 | 서버 메모리 | 기본 1시간 |
| 사고 블록 복원용 기록 | 합니다 | 서버 메모리(Home 모드에서는 Home 저장소) | 1시간 |
| 연결과 TLS 세션 | 합니다 | 서버 메모리 | 프록시별 상한 있음 |

**주의:** 서버가 가진 캐시는 모두 메모리에만 있습니다. 서버를 재시작하면 사라집니다. 프롬프트 캐시는 제공자 쪽에 있으므로 재시작해도 남지만, 대화가 다른 계정으로 옮겨 가면 쓸 수 없습니다.

<a id="prompt-cache"></a>
## 제공자의 프롬프트 캐시 살리기

제공자는 계정마다 프롬프트 캐시를 따로 둡니다. 같은 대화의 다음 요청이 같은 계정으로 가면 앞부분을 캐시에서 읽습니다. 한도를 덜 쓰고 답이 빨리 시작됩니다. 다른 계정으로 가면 캐시를 다시 만들어야 합니다.

그래서 프록시가 하는 일은 두 가지입니다. 요청에 캐시 표시를 맞추는 일과, 대화를 계정에 묶는 일입니다.

<a id="claude-cache-control"></a>
### Claude: 캐시 표시(`cache_control`) 맞추기

Anthropic API에서는 요청 본문에 `cache_control` 표시를 붙인 위치까지를 캐시 대상으로 삼습니다. 프록시는 이 표시를 다음 규칙으로 다룹니다(`crates/horon-exec/src/claude.rs`, `crates/horon-exec/src/claude/cloak.rs`).

1. **프록시가 표시를 맡을지 정합니다.** 요청이 Claude Code 본체에서 온 것으로 확인되면(`x-app=cli` 같은 신호가 모두 맞을 때) 클라이언트의 표시를 그대로 둡니다. 그렇지 않고 표시가 하나도 없거나 시스템 프롬프트를 다시 쓴 경우에는 프록시가 표시를 넣습니다.
2. **표시를 넣는 위치(`ensure_cache_control`):**
   - 시스템 프롬프트가 있으면 시스템 프롬프트의 마지막 블록에 넣습니다. 이미 표시가 있으면 넣지 않습니다.
   - 시스템 프롬프트가 비어 있으면 도구(tools) 목록의 마지막 도구에 넣습니다. `defer_loading`인 도구는 건너뜁니다.
   - 대화(messages)에서는 마지막으로 표시할 수 있는 메시지의 마지막 블록에 넣습니다. 끝이 `thinking`이나 `redacted_thinking`인 어시스턴트 메시지는 건너뜁니다.
3. **표시는 최대 4개입니다(`enforce_cache_limit`).** 4개를 넘으면 시스템과 도구의 앞쪽 표시부터, 그다음 메시지의 표시, 마지막으로 남은 시스템과 도구의 표시 순서로 지웁니다.
4. **유지 시간(TTL) 표시를 맞춥니다:**
   - Claude Code 프로필로 보내는 요청(OAuth 로그인 계정)에서 프록시가 표시를 맡았다면 유지 시간을 `1h`(1시간)로 올립니다(`upgrade_ttl`). 하위 에이전트(subagent) 요청은 1시간을 요청했을 때만 올립니다.
   - 그 외 하위 에이전트 요청과 탐색용 요청에서는 유지 시간 지정을 지웁니다(`strip_ttl`).
   - 마지막으로 `normalize_ttl`이 `5m` 표시 뒤에 `1h` 표시가 오지 않도록 정리합니다. 제공자가 이 순서를 허용하지 않기 때문입니다.

이 작업은 본문을 한 번 훑으면서 처리합니다. 본문이 커져도 작업량이 크기에 비례하게 하려는 설계입니다(`docs/BENCHMARKS.md`에 측정 결과가 있습니다).

<a id="codex-prompt-cache-key"></a>
### Codex와 OpenAI 호환 제공자: `prompt_cache_key` 넣기

OpenAI 계열은 요청의 `prompt_cache_key` 값이 같으면 같은 캐시를 쓰도록 안내합니다. 프록시는 이 값을 대화마다 안정적으로 만듭니다.

**Codex 계정(`crates/horon-exec/src/codex_request.rs`의 `prompt_cache`):** 다음 순서로 첫 번째로 나오는 값을 씁니다.

1. 클라이언트가 보낸 `prompt_cache_key`(Responses 형식, 그리고 WebSocket이 아닌 Chat 형식).
2. Claude Code에서 온 요청이면 Claude Code 세션과 모델로 만든 고정 UUID.
3. 서버가 파악한 세션 식별자로 만든 UUID.
4. 위 세 가지가 모두 없고 Chat 형식이며 WebSocket이 아닐 때는 호출한 클라이언트 키(principal)로 만든 UUID.

값을 못 만들면 `prompt_cache_key`를 넣지 않습니다.

**OpenAI 호환 제공자(`crates/horon-exec/src/openai_compat.rs`):** 제공자 항목에 `support-prompt-cache-key: true`를 켠 경우에만 넣습니다. 켜지 않으면 본문을 건드리지 않습니다. 켜면 같은 순서로 값을 정합니다. 클라이언트가 보낸 값이 있으면 그대로 쓰고, Claude Code 요청이면 그 세션으로 만들고, 아니면 실행 세션(없으면 파생 세션)과 제공자, 모델, 요청 형식으로 UUID를 만듭니다.

```yaml
api-keys:
  openai-compatibility:
    - name: openrouter
      base-url: "https://openrouter.ai/api/v1"
      support-prompt-cache-key: true
```

위 설정 예시는 키를 켜는 위치를 보여 주기 위한 것입니다. 제공자가 `prompt_cache_key`를 모르는 경우에는 켜지 않습니다.

<a id="command-code"></a>
### Command Code: 캐시 표시 없이 세션 ID만 보냅니다

Command Code 계정(`command-code` 제공자)은 위 두 방식 중 어느 것도 쓰지 않습니다(`crates/horon-exec/src/command_code.rs`).

- **캐시 표시를 넣지 않습니다.** 어떤 형식의 요청이 와도 OpenAI Chat 형식으로 바꾼 뒤 Command Code의 `/alpha/generate` 본문으로 다시 만듭니다. 이 본문에는 `cache_control`도 `prompt_cache_key`도 들어가지 않습니다. 클라이언트가 보낸 `prompt_cache_key`도 이 본문에는 전달되지 않습니다.
- **세션 ID를 헤더로 보냅니다.** 요청마다 `x-session-id` 헤더를 붙입니다. 값은 실행 세션, 세션, 파생 세션 중 처음으로 값이 있는 것에서 만든 고정 UUID입니다. 같은 대화는 같은 값을 받습니다. 세 값이 모두 없으면 요청마다 새 UUID를 만듭니다.
- **캐시가 실제로 쓰였는지는 응답으로 확인합니다.** 응답의 `inputTokenDetails.cacheReadTokens`를 `prompt_tokens_details.cached_tokens`로 옮겨 사용 기록에 담습니다. 이 제공자의 토큰 집계는 캐시 토큰을 입력 토큰 안에 포함하는 방식(포함형)으로 읽습니다([사용량과 한도 조회](USAGE.md#how-tokens-are-counted)).
- **캐시 읽기만 집계합니다.** 캐시 쓰기 토큰은 응답에서 읽지 않으므로 `cache_creation_tokens`는 0으로 남습니다.
- **토큰 세기는 서버 안에서 합니다.** `POST /v1/messages/count_tokens`는 업스트림에 보내지 않고 tiktoken 방식으로 직접 셉니다. 이때 캐시는 관여하지 않습니다.

**주의:** 이 제공자의 캐시는 Command Code 서버가 알아서 하는 일이고, 프록시가 켜거나 조절하는 설정은 없습니다. 위 `x-session-id`가 캐시 적중에 얼마나 영향을 주는지는 제공자 문서나 코드로 확인하지 못했습니다. 그래서 대화를 같은 계정에 묶는 세션 고정(아래 절)을 켜 두는 것이 안전합니다.

<a id="session-affinity-as-cache-protection"></a>
### 세션 고정이 캐시를 지킵니다

프롬프트 캐시를 쓰려면 같은 대화가 같은 계정으로 가야 합니다. 세션 고정(session affinity)이 이 일을 합니다. 설정과 사용법은 [여러 계정](MULTI-ACCOUNT.md#session-affinity)에 있고, 이 절에서는 캐시 관점에서만 설명합니다.

대화를 알아보는 순서는 다음과 같습니다(`crates/horon-common/src/session.rs`).

1. 클라이언트가 보낸 명시적 세션 식별자. Claude Code, Codex, OpenCode 같은 도구가 헤더나 본문에 넣어 보냅니다.
2. 파생 식별자와 첫 메시지 해시 같은 대체 값.
3. 위 값이 없는 요청은 대화 내용의 앞부분으로 찾습니다. 메시지를 정규화한 뒤 앞에서부터 겹치는 부분을 해시로 비교해(Merkle 방식의 최장 공통 접두사 매칭) 어느 계정이 처리한 대화인지 기억합니다. 대화가 이어지거나 갈라지거나 압축되어도 같은 계정을 유지합니다(`crates/horon-server/src/lcp.rs`).

찾은 대화는 `(제공자 범위, 모델, 세션 ID)`를 열쇠로 계정에 묶입니다(`crates/horon-server/src/affinity.rs`).

- 한 계정에 여러 세션 열쇠를 묶어 한 그룹으로 다룹니다. 열쇠 하나를 쓰면 그룹 전체의 만료 시각이 늘어납니다.
- 그룹마다 `pck:`로 시작하는 프롬프트 캐시 별칭을 하나만 두고, 그 밖의 별칭은 64개까지만 둡니다.
- 항목은 65,536개까지입니다. 넘으면 먼저 만든 그룹부터 지웁니다.
- 묶인 계정이 쿨다운에 들어가거나 비활성화되면 대화가 다른 계정으로 옮겨 갑니다. 이때 제공자 쪽 프롬프트 캐시는 버려집니다.

**주의:** 세션 고정을 켜지 않으면 매 요청이 라우팅 방식(`routing.strategy`)에 따라 계정을 새로 고릅니다. 이 경우 같은 대화가 계정을 오가므로 프롬프트 캐시를 거의 쓰지 못합니다. 서버 기본값은 꺼짐이고, 설치 스크립트는 새 설정에 켜 둡니다.

<a id="cache-tokens-in-usage"></a>
### 캐시 토큰 집계

제공자가 응답에 캐시 읽기와 캐시 쓰기 토큰 수를 알려 주면 서버가 사용 기록에 따로 담습니다(`cache_read_tokens`, `cache_creation_tokens`, `cached_tokens`). 캐시가 실제로 맞았는지는 이 값으로 확인합니다. 자세한 내용은 [사용량과 한도 조회](USAGE.md#usage-records)에 있습니다.

<a id="in-process-caches"></a>
## 서버 안의 상태 캐시

아래 캐시는 모두 서버 메모리에 있고, 보관 시간과 크기에 한도가 있습니다. 대부분 성능이 아니라 **정확성**을 위한 것입니다.

| 캐시 | 용도 | 보관 시간 | 크기 한도 | 위치 |
|---|---|---|---|---|
| 세션과 계정의 연결 | 대화를 같은 계정에 묶음 | `routing.session-affinity-ttl`, 기본 1시간 | 65,536항목 | `crates/horon-server/src/affinity.rs` |
| 대화 접두사 매처 | 세션 ID가 없는 대화를 알아봄 | 1시간 | 그룹 4,096개, 접두사 항목 262,144개, 대화당 최대 1,024턴이 기본값 | `crates/horon-server/src/lcp.rs` |
| Home 모드 세션 별칭 | Home에서의 세션 연결 | 기본 1시간, 설정한 TTL이 바뀌면 전체를 비움 | 소프트 상한 초과 시 오래된 그룹부터 지움 | `crates/horon-home/src/session_alias.rs` |
| Codex 추론 복원 | 이전 턴의 추론 항목을 다음 요청에 복원 | 1시간(읽으면 연장) | 10,240항목, 항목당 턴 256개와 16 MiB | `crates/horon-exec/src/codex_replay.rs` |
| Kimi 사고 복원 | 서명된 사고 블록을 다음 턴에 복원 | 1시간 | 10,240항목, 전체 256 MiB, 항목당 8 MiB | `crates/horon-exec/src/kimi_replay.rs` |
| Claude API 키 사고 복원 | 서명된 사고 블록을 다음 턴에 복원 | 1시간, 10분마다 정리 | 세션당 64턴, 턴당 8 MiB와 512블록 | `crates/horon-exec/src/claude/replay.rs`, `crates/horon-exec/src/replay.rs` |
| Antigravity 복원과 서명 | 생각 서명을 다시 붙임 | 1시간(쓰면 연장) | 10,240항목, 넘으면 128개씩 지움 | `crates/horon-translate/src/replay_cache.rs` |
| 모델 카탈로그 | 모델 정보 | 원격에서 새 목록이 오면 교체 | 한 벌 | `crates/horon-core/src/registry.rs` |
| 릴리스 확인 | 새 버전 확인 결과 | 12시간 | 태그 하나 | `crates/horon-server/src/management/observability.rs` |
| 비디오 결과 계정 연결 | 비디오 조회가 같은 xAI 계정으로 가게 함 | `multimedia.video-result-auth-cache-ttl` | 해당 없음 | `crates/horon-server/src/videos.rs` |

**인증 디렉터리 리로드:** 설정 파일 감시자가 auth-dir의 `*.json`을 다시 읽을 때, 파일별 메타데이터(길이, inode, 수정 시각, 변경 시각)를 비교해 **네 값이 모두 같고 마지막 수정 후 2초가 지난 파일은 다시 읽지도 파싱하지도 않습니다**(`crates/horon-core/src/config/credentials.rs`). 감시자와 같은 규칙입니다. 네 값이 같은 채 내용만 바뀌는 경우는 감지하지 못하지만, 정상적인 편집은 수정 시각을 갱신하므로 발생하지 않습니다. 전역 제외 모델 목록처럼 설정만 바뀐 경우에는 파일을 다시 읽지 않고 기억해 둔 내용으로 인증 정보만 다시 만듭니다.

<a id="why-thinking-replay-records-exist"></a>
### 사고 블록 복원 기록이 필요한 이유

Claude Code 같은 클라이언트는 대화 기록을 보낼 때 서명된 `thinking` 블록을 빼는 경우가 있습니다. 일부 업스트림은 도구를 호출한 턴에 이 블록이 없으면 요청을 거부합니다. 그래서 서버는 응답에 들어 있던 어시스턴트 내용(서명된 사고와 도구 호출)을 (모델 계열, 세션) 단위로 기억해 두었다가 다음 요청의 해당 턴에 되돌려 넣습니다(`crates/horon-exec/src/replay.rs`). 이 기록은 속도를 위한 캐시가 아닙니다. 없으면 요청이 실패할 수 있습니다.

기록이 한도를 넘거나 만료되면 해당 항목을 지웁니다. 이 경우 복원하지 못해서 업스트림이 거부할 수 있으므로, 한 세션을 1시간 넘게 쉬었다가 이어 가는 경우에 주의합니다.

<a id="in-home-mode"></a>
### Home 모드에서

Home 모드(`-home-jwt`)에서는 사고 복원 기록이 서버 메모리 대신 Home의 공유 저장소(KV)로 갑니다. 읽을 때 만료 시간을 갱신하고, 쓸 때는 비교 후 교체(compare-and-swap) 방식으로 다른 쓰기와 겹치지 않게 합니다. Home이 관리하는 쪽의 규칙은 [호환성 현황](PARITY.md)을 참고합니다.

<a id="how-entries-are-cleaned-up"></a>
### 정리 방식

- 만료된 항목은 읽을 때 지웁니다.
- 일부 캐시는 10분 간격으로 한꺼번에 정리합니다. 별도 타이머 없이 다음 접근 때 기한이 지난 정리 시각을 따라잡아 실행합니다.
- 세션 대화 접두사 매처는 백그라운드 작업 없이 접근할 때만 정리합니다.

<a id="connection-caches"></a>
## 연결과 TLS 세션 재사용

- **연결 재사용:** Claude 전용 클라이언트와 표준 HTTP 클라이언트가 요청 사이에 연결을 유지합니다. 표준 전송은 호스트당 유휴 연결을 최대 2개 둡니다.
- **프록시별 클라이언트:** 클라이언트는 실제로 쓰는 프록시 주소별로 하나씩 두고 최근에 쓴 64개만 유지합니다(`crates/horon-exec/src/proxy.rs`, `crates/horon-exec/src/tls.rs`).
- **TLS 세션 재개:** Claude 클라이언트마다 32개의 TLS 세션 캐시를 따로 가집니다. 프록시가 다르면 세션을 공유하지 않습니다.
- **Codex 연결 유지:** `providers.codex.chatgpt-keep-alive: true`로 켜면 `chatgpt.com`과의 유휴 연결을 프록시당 최대 2개, 90초 동안 유지합니다. 기본값은 꺼짐입니다. 설정 설명은 [설정](CONFIGURATION.md#oauth)과 [CLIProxyAPI와 다른 점](DIFFERENCES-FROM-GO.md)에 있습니다.

<a id="management-responses"></a>
## 관리 화면 응답

관리 API와 대시보드 응답에는 브라우저가 저장하지 않도록 `Cache-Control: no-store`를 붙입니다. 정적 대시보드 파일 일부는 `no-cache`입니다(`crates/horon-server/src/lib.rs`, `crates/horon-server/src/management.rs`). 인증 정보나 계정 상태가 브라우저 캐시에 남지 않게 하려는 규칙입니다.

<a id="what-is-not-cached"></a>
## 캐시하지 않는 것

- **AI 응답.** 같은 요청도 매번 업스트림에 보냅니다. 클라이언트 키별, 모델별 응답 캐시는 없습니다.
- **의미 기반(시맨틱) 캐시.** 비슷한 질문을 같은 질문으로 보지 않습니다.
- **디스크 캐시.** 서버 재시작 후에 남는 캐시는 없습니다. 디스크에 남는 것은 `save-cooldown-status`를 켠 경우의 쿨다운 기록과 요청 로그 파일뿐이고, 둘 다 캐시가 아닙니다.
- **요청 본문.** 이식한 규칙이 본문을 고치는 경우를 제외하면 본문은 바이트 그대로 전달합니다. 표시 맞추기(위 Claude 절)와 `prompt_cache_key` 넣기는 그 이식 규칙에 해당합니다.

<a id="memory-note"></a>
## 메모리에 관한 참고

세션 대화 접두사 매처는 항목 수에는 한도가 있지만 바이트 수에는 한도가 없습니다. 세션 ID 없이 긴 대화를 여러 개 동시에 돌리면 메모리가 늘 수 있습니다. 소스 주석에 측정값이 있습니다. 150턴짜리 대화 4개가 3초마다 요청을 보냈을 때 1시간 뒤 힙이 57.7 MB였고, 300턴짜리 대화에서는 40분 뒤부터 82.2 MB였습니다(`crates/horon-server/src/lcp.rs`). 개인용 규모에서는 문제 되지 않는 수준이지만, 세션 ID를 보내는 도구를 쓰면 이 매처를 거치지 않습니다.
