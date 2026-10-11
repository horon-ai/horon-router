# HyperRouter를 도구와 함께 쓰기

> 이 문서에서 알 수 있는 것: Claude Code, Codex CLI, Gemini CLI 같은 도구와 SDK를 hyper-router에 연결하는 방법.

예제는 `http://127.0.0.1:8317`과 `your-client-key`를 씁니다. 두 값을 `config.yaml`에 있는 주소와 클라이언트 키(도구가 프록시에 접속할 때 쓰는 비밀 키)로 바꿉니다. `/management.html`의 대시보드에는 **Use with tools** 페이지가 있고, 여기서 두 값을 복사 버튼과 함께 볼 수 있습니다. 모델 ID는 대시보드의 **Models** 페이지나 `GET /v1/models`에서 고릅니다. 쓸 수 있는 ID는 프록시(내 요청을 대신 전달해 주는 중간 서버)에 연결한 계정에 따라 다릅니다.

<a id="claude-code"></a>
## Claude Code

[Claude Code의 게이트웨이 안내](https://code.claude.com/docs/en/llm-gateway-connect)는 Anthropic Messages 형식을 씁니다. 기본 URL은 `/v1` 없이 설정합니다. Claude Code가 `/v1/messages`를 붙이기 때문입니다. 토큰(로그인 증명서 같은 값)은 bearer 인증으로 전송됩니다. 원하면 `ANTHROPIC_MODEL`에 프록시의 모델 ID를 설정합니다.

```bash
export ANTHROPIC_BASE_URL=http://127.0.0.1:8317
export ANTHROPIC_AUTH_TOKEN=your-client-key
export ANTHROPIC_MODEL=<model-id>
claude
```

설정을 계속 쓰려면 `~/.claude/settings.json`에 아래 내용을 넣습니다.

```json
{
  "env": {
    "ANTHROPIC_BASE_URL": "http://127.0.0.1:8317",
    "ANTHROPIC_AUTH_TOKEN": "your-client-key",
    "ANTHROPIC_MODEL": "<model-id>"
  }
}
```

<a id="claude-code-versions"></a>
### Claude Code 버전

Claude 계정을 쓰면 Claude Code가 2.1.280 이상(주 버전 2 안)일 때 hyper-router가 Claude Code의 고유 신원(`User-Agent`와 SDK, Node 버전)을 그대로 전달합니다. 2.1.280보다 새 버전은 Claude Code처럼 `X-Stainless-Package-Version`과 `X-Stainless-Runtime-Version` 헤더를 둘 다 보내야 합니다. 세션 제목 요청과 사용 한도 확인 요청도 같은 방식으로 전달됩니다. `stabilize-device-profile: true`이면 2.1.280부터의 2.1.x 버전도 Claude Code로 계속 인정되어 신원을 감추지 않습니다. 다만 고정된 2.1.280 신원으로 업스트림(요청을 실제로 받아 처리하는 제공자 서버)에 전달됩니다. 2.2 이상은 신원을 감추고 Stainless 요구 사항을 적용하지 않습니다. 다른 클라이언트와 더 오래된 Claude Code는 Claude Code 2.1.280으로 업스트림에 전달됩니다. `claude-cli` 클라이언트를 그대로 넘기지 않을 때는 계정과 버전마다 로그에 한 번만 남기며, 계정은 인증 번호(대시보드와 관리 API가 보여 주는 값)로 표시합니다. Stainless 헤더가 빠져서인 경우에는 그 이유도 함께 남깁니다.

```text
claude: Claude Code 2.1.220 on credential 1f3a9c0b7d2e4a68 is not passed through; requests use the claude-cli/2.1.280 (external, cli) identity. ...
```

Anthropic은 모델에 비해 너무 오래된 Claude Code 버전을 거부합니다. 이 오류는 400으로 클라이언트에 전달됩니다.

```text
Claude Code 2.1.236 does not support this model; version 2.1.251 or newer is required. Run 'claude update', or update the Claude desktop app, then try again.
```

`error.details.error_code`는 `claude_code_version_too_old`입니다. Claude Code 자체에서는 `claude update`를 실행합니다. 다른 클라이언트를 쓰거나 전달되지 않는 Claude Code 버전을 쓰면 hyper-router가 보내는 신원이 모델이 요구하는 버전보다 낮습니다. hyper-router 릴리스에서 이 값을 올리기 전까지는 `config.yaml`에서 더 새 버전을 설정합니다. `claude --version`이 출력하는 버전과 그 버전이 보내는 `X-Stainless-Package-Version`, `X-Stainless-Runtime-Version` 헤더를 씁니다.

```yaml
oauth:
  providers:
    claude:
      header-defaults:
        user-agent: "claude-cli/<version> (external, cli)"
        package-version: "<X-Stainless-Package-Version>"
        runtime-version: "<X-Stainless-Runtime-Version>"
```

예전 구조에서는 같은 블록을 최상위 수준에 `claude-header-defaults:`로 씁니다. `user-agent`에 적은 버전은 그 버전 자체로 전달되는 가장 낮은 Claude Code 버전이기도 합니다.

<a id="gpt-in-claude-code"></a>
### Claude Code에서 GPT 쓰기

OpenAI API 키(프로그램이 쓰는 비밀번호)를 설정하고 `/v1/models`에서 쓸 수 있는 GPT 모델을 고릅니다. 연결한 Codex 계정을 써도 됩니다. 먼저 [계정과 제공자 이용약관](../README.md#accounts-and-provider-terms)을 읽습니다. Claude Code는 Messages 요청을 보내고, 프록시가 이를 선택한 제공자의 형식으로 변환합니다.

```bash
export ANTHROPIC_BASE_URL=http://127.0.0.1:8317
export ANTHROPIC_AUTH_TOKEN=your-client-key
export ANTHROPIC_MODEL='<gpt-model-id>'
export ANTHROPIC_DEFAULT_HAIKU_MODEL='<gpt-model-id>'
# 컨텍스트 창이 200,000 토큰 이상인 모델을 쓸 때의 예입니다:
export CLAUDE_CODE_MAX_CONTEXT_TOKENS=200000
export CLAUDE_CODE_AUTO_COMPACT_WINDOW=180000
claude
```

`ANTHROPIC_DEFAULT_HAIKU_MODEL`은 Claude Code가 보내는 작은 모델 호출도 이쪽으로 보냅니다. 이 값을 설정하지 않으면 여전히 Haiku를 요청할 수 있습니다. 그 자리에 다른 GPT 모델을 써도 됩니다. Claude Code의 이름 있는 모델 등급을 쓴다면 `ANTHROPIC_DEFAULT_OPUS_MODEL`, `ANTHROPIC_DEFAULT_SONNET_MODEL`, `ANTHROPIC_DEFAULT_FABLE_MODEL`도 프록시에서 쓸 수 있는 모델로 설정합니다.

고른 모델의 제공자 사용 한도를 확인하고 그보다 낮은 압축 창을 씁니다. Claude Code의 [환경 변수 안내](https://code.claude.com/docs/en/env-vars)에 두 변수가 정의되어 있습니다. `CLAUDE_CODE_MAX_CONTEXT_TOKENS`는 Claude Code가 가정하는 창 크기를 정합니다. 업스트림의 컨텍스트 한도는 그대로 적용됩니다. `CLAUDE_CODE_AUTO_COMPACT_WINDOW`는 100,000에서 1,000,000 사이의 정수 토큰 수를 받습니다. 두 변수는 Claude Code 환경에 설정합니다.

<a id="codex-cli"></a>
## Codex CLI

[Codex 사용자 정의 모델 제공자](https://developers.openai.com/codex/config-advanced)는 OpenAI Responses 형식을 씁니다. `~/.codex/config.toml`에 아래 내용을 넣고, 환경 변수를 설정하고, `<model-id>`를 바꿉니다. WebSocket 지원도 문서에 있지만 선택 사항이므로 이 설정은 일반 HTTP 엔드포인트를 씁니다.

```toml
model = "<model-id>"
model_provider = "hyperrouter"

[model_providers.hyperrouter]
name = "hyper-router"
base_url = "http://127.0.0.1:8317/v1"
env_key = "HYPERROUTER_CLIENT_KEY"
wire_api = "responses"
```

```bash
export HYPERROUTER_CLIENT_KEY=your-client-key
codex
```

<a id="gemini-cli"></a>
## Gemini CLI

[Gemini CLI 공식 저장소](https://github.com/google-gemini/gemini-cli/issues/1679#issuecomment-3293504913)에서 사용자 정의 Gemini 엔드포인트용 `GOOGLE_GEMINI_BASE_URL`을 확인할 수 있습니다. 키 변수는 문서에 나온 대로 `GEMINI_API_KEY`입니다. CLI는 Gemini `/v1beta` 경로를 씁니다.

```bash
export GOOGLE_GEMINI_BASE_URL=http://127.0.0.1:8317
export GEMINI_API_KEY=your-client-key
gemini
```

`<model-id>`는 Gemini CLI의 `/model` 명령으로 고릅니다. 공식 문서에서 확인되지 않음: 사용자 정의 엔드포인트용 모델 환경 변수.

<a id="amp"></a>
## Amp

[Amp 모델 라우팅](https://ampcode.com/docs/customize/model-routing#custom-url-connections)은 Amp 서버에서 Custom URL 연결을 호출합니다. 먼저 프록시를 공개 HTTPS 주소로 노출합니다. Model Routing에서 Add를 고르고 Custom URL을 선택합니다. 아래 설정 중 하나를 쓰고 Amp 모델을 `<model-id>`에 연결합니다.

```text
Anthropic Messages
API format: anthropic-messages
Base URL: https://proxy.example.com
API key: your-client-key

OpenAI Responses
API format: responses
Base URL: https://proxy.example.com/v1
API key: your-client-key
```

API 키는 bearer 인증으로 전송됩니다. Check Access를 쓰거나 다음을 실행합니다.

```bash
amp config model-providers check-access --provider-model <provider/model>
```

<a id="opencode"></a>
## OpenCode

[OpenCode 제공자 안내](https://opencode.ai/docs/providers#custom-provider)에 사용자 정의 OpenAI 호환 제공자가 나옵니다. `opencode.json`에 아래 내용을 추가하고 `<model-id>`를 바꾼 다음 `/models`에서 `hyperrouter/<model-id>`를 선택합니다. OpenAI Chat Completions를 씁니다.

```json
{
  "$schema": "https://opencode.ai/config.json",
  "provider": {
    "hyperrouter": {
      "npm": "@ai-sdk/openai-compatible",
      "name": "hyper-router",
      "options": {
        "baseURL": "http://127.0.0.1:8317/v1",
        "apiKey": "your-client-key"
      },
      "models": {
        "<model-id>": { "name": "<model-id>" }
      }
    }
  }
}
```

<a id="factory-droid"></a>
## Factory Droid

[Factory 사용자 정의 모델 안내](https://docs.factory.ai/model-independence/byok)는 `~/.factory/settings.json`을 씁니다. 이 예제는 OpenAI Chat Completions를 씁니다. `<model-id>`를 바꾸고 `/model`로 선택합니다.

```json
{
  "customModels": [
    {
      "model": "<model-id>",
      "displayName": "hyper-router",
      "baseUrl": "http://127.0.0.1:8317/v1",
      "apiKey": "your-client-key",
      "provider": "generic-chat-completion-api"
    }
  ]
}
```

Anthropic Messages를 쓰려면 대신 `provider: "anthropic"`과 `baseUrl: "http://127.0.0.1:8317"`을 씁니다.

<a id="cline-roo-code-and-kilo-code"></a>
## Cline, Roo Code, Kilo Code

[Cline](https://docs.cline.bot/provider-config/openai-compatible), [Roo Code](https://docs.roocode.com/providers/openai-compatible), [Kilo Code](https://kilo.ai/docs/ai-providers/openai-compatible)는 OpenAI Chat Completions 필드가 같습니다. 확장 기능의 제공자 설정을 열고 아래 값을 넣습니다.

```text
Provider or provider API: OpenAI Compatible
Base URL: http://127.0.0.1:8317/v1
API key: your-client-key
Model or model ID: <model-id>
```

Kilo에는 OpenAI Responses와 Anthropic Messages 사용자 정의 제공자 형식도 있습니다. Cline은 Anthropic Use custom base URL 옵션을 문서에 적어 두었습니다. Roo Code의 인용된 페이지는 OpenAI 호환 설정만 확인하고 있습니다.

<a id="cursor"></a>
## Cursor

[Cursor의 현재 키 안내](https://cursor.com/help/models-and-usage/api-keys)는 Cursor Settings와 Models, OpenAI key 항목을 확인합니다. 또 모든 요청이 Cursor 서버를 거친다고 설명하므로 `127.0.0.1`은 쓸 수 없습니다. 먼저 프록시를 HTTPS로 노출합니다.

```text
Cursor Settings > Models
OpenAI API key: your-client-key
Override OpenAI Base URL: https://proxy.example.com/v1
Custom model: <model-id>
```

공식 문서에서 확인되지 않음: 현재 공식 안내에는 Override OpenAI Base URL과 Custom model 항목이 없습니다. 이 설정을 쓰기 전에 설치한 Cursor 버전에 두 항목이 있는지 확인합니다.

<a id="zed"></a>
## Zed

[Zed API 접근 안내](https://zed.dev/docs/ai/use-api-access#openai-compatible)는 사용자 정의 OpenAI 호환 제공자를 지원합니다. Zed의 `settings.json`에 아래 내용을 넣고 `<model-id>`를 바꾸고, 생성한 키 변수를 설정합니다. Zed는 기본으로 Chat Completions를 씁니다.

```json
{
  "language_models": {
    "openai_compatible": {
      "hyperrouter": {
        "api_url": "http://127.0.0.1:8317/v1",
        "available_models": [
          {
            "name": "<model-id>",
            "display_name": "hyper-router",
            "max_tokens": 128000
          }
        ]
      }
    }
  }
}
```

```bash
export HYPERROUTER_API_KEY=your-client-key
```

OpenAI Responses를 쓰려면 모델 항목에서 `capabilities.chat_completions`를 `false`로 설정합니다. Zed는 Anthropic Messages용 `language_models.anthropic_compatible`과 `api_url: "http://127.0.0.1:8317"`도 문서에 적어 두었습니다.

<a id="continue"></a>
## Continue

[Continue OpenAI 제공자 안내](https://docs.continue.dev/customize/model-providers/top-level/openai#openai-api-compatible-providers)는 `config.yaml`의 `apiBase`, `apiKey`, `model`을 씁니다. 이 설정은 OpenAI Chat Completions를 씁니다.

```yaml
name: hyper-router
version: 1.0.0
schema: v1
models:
  - name: hyper-router
    provider: openai
    model: <model-id>
    apiBase: http://127.0.0.1:8317/v1
    apiKey: your-client-key
```

<a id="aider"></a>
## Aider

[Aider OpenAI 호환 안내](https://aider.chat/docs/llms/openai-compat.html)는 이 변수들과 `openai/` 모델 접두사를 문서에 적어 두었습니다. OpenAI 호환 요청을 씁니다.

```bash
export OPENAI_API_BASE=http://127.0.0.1:8317/v1
export OPENAI_API_KEY=your-client-key
aider --model openai/<model-id>
```

공식 문서에서 확인되지 않음: Anthropic 사용자 정의 base 변수. Aider는 `ANTHROPIC_API_KEY`를 문서에 적어 두었지만, 현재 옵션 레퍼런스에는 `ANTHROPIC_API_BASE`가 없습니다.

<a id="sdks"></a>
## SDK

모든 예제에서 `<model-id>`는 프록시의 모델 ID로 바꿉니다.

<a id="openai-python"></a>
### OpenAI Python

[공식 Python SDK](https://github.com/openai/openai-python)는 여기서 OpenAI Responses를 씁니다.

```python
from openai import OpenAI

client = OpenAI(base_url="http://127.0.0.1:8317/v1", api_key="your-client-key")
response = client.responses.create(model="<model-id>", input="Hello")
print(response.output_text)
```

<a id="openai-javascript"></a>
### OpenAI JavaScript

[공식 JavaScript SDK](https://github.com/openai/openai-node/blob/main/docs/authentication.md)는 camelCase인 `baseURL`과 `apiKey`를 씁니다.

```javascript
import OpenAI from "openai";

const client = new OpenAI({
  baseURL: "http://127.0.0.1:8317/v1",
  apiKey: "your-client-key",
});
const response = await client.responses.create({ model: "<model-id>", input: "Hello" });
console.log(response.output_text);
```

<a id="anthropic-python"></a>
### Anthropic Python

[공식 Python SDK](https://github.com/anthropics/anthropic-sdk-python)는 Anthropic Messages와 `/v1`이 없는 기본 URL을 씁니다.

```python
import anthropic

client = anthropic.Anthropic(base_url="http://127.0.0.1:8317", api_key="your-client-key")
message = client.messages.create(
    model="<model-id>", max_tokens=256,
    messages=[{"role": "user", "content": "Hello"}],
)
print(message.content[0].text)
```

<a id="anthropic-javascript"></a>
### Anthropic JavaScript

[공식 JavaScript SDK](https://github.com/anthropics/anthropic-sdk-typescript)는 `baseURL`과 `apiKey`를 씁니다.

```javascript
import Anthropic from "@anthropic-ai/sdk";

const client = new Anthropic({
  baseURL: "http://127.0.0.1:8317",
  apiKey: "your-client-key",
});
const message = await client.messages.create({
  model: "<model-id>", max_tokens: 256,
  messages: [{ role: "user", content: "Hello" }],
});
console.log(message.content[0].text);
```

<a id="google-gen-ai-python"></a>
### Google Gen AI Python

[공식 Python SDK 소스](https://github.com/googleapis/python-genai/blob/main/google/genai/client.py)는 `api_key`와 `http_options`를 받습니다. Gemini `/v1beta` 경로를 씁니다.

```python
from google import genai
from google.genai import types

client = genai.Client(
    api_key="your-client-key",
    http_options=types.HttpOptions(base_url="http://127.0.0.1:8317"),
)
response = client.models.generate_content(model="<model-id>", contents="Hello")
print(response.text)
```

공개된 Python 안내는 사용자 정의 기본 URL을 엔터프라이즈 모드에서만 설명합니다. SDK의 현재 소스는 Gemini Developer API에서도 위 조합을 받습니다.

<a id="google-gen-ai-javascript"></a>
### Google Gen AI JavaScript

[공식 JavaScript SDK 소스](https://github.com/googleapis/js-genai/blob/main/src/client.ts)는 `apiKey`와 `httpOptions.baseUrl`을 받습니다.

```javascript
import { GoogleGenAI } from "@google/genai";

const client = new GoogleGenAI({
  apiKey: "your-client-key",
  httpOptions: { baseUrl: "http://127.0.0.1:8317" },
});
const response = await client.models.generateContent({
  model: "<model-id>", contents: "Hello",
});
console.log(response.text);
```

<a id="check-it-works"></a>
## 동작 확인

```bash
curl -sS http://127.0.0.1:8317/v1/models \
  -H 'Authorization: Bearer your-client-key'
```

`401` 응답이 오면 클라이언트 키가 틀린 것입니다. 클라우드에서 호스팅하는 도구가 연결하지 못하면 `127.0.0.1` 대신 `https://proxy.example.com` 같은 공개 HTTPS 주소를 씁니다.
