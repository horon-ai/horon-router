# 코딩 에이전트로 HyperRouter 설치하기

> 이 문서에서 알 수 있는 것: 코딩 에이전트가 사용자 컴퓨터에 hyper-router를 설치하고 처음 실행하는 방법.

이 문서는 사용자 컴퓨터에 hyper-router를 설치하는 코딩 에이전트(Claude Code, Codex, Amp, Cursor의 에이전트 등)를 위한 정확한 절차입니다. 사람이 따라 해도 됩니다. 모든 명령은 사용자 컴퓨터에서 순서대로 실행하고, 단계에서 사용자에게 물어보라고 한 곳에서는 멈추고 물어보십시오.

<a id="rules-for-the-agent"></a>
## 에이전트가 지킬 규칙

- 사용자 대신 제공자 계정에 로그인하지 않습니다. 사용자의 비밀번호를 입력하거나 붙여넣거나 읽지도 않습니다. 로그인은 사용자가 할 일입니다(7단계).
- 관리 키와 클라이언트 키를 출력하거나 로그에 남기거나 다시 옮겨 적지 않습니다. 두 키는 `~/.hyper-router/keys.env`에 있으니 그 파일을 가리키십시오.
- 사용자가 옮겨 달라고 요청하지 않는 한, 이미 있는 CLIProxyAPI(Go)나 hyper-router 설치를 멈추거나 바꾸거나 다시 설정하지 않습니다(이 문서 끝을 보십시오).
- **주의:** 프록시를 인터넷에 열지 않습니다. `server.host`를 `127.0.0.1`로 유지합니다.
- 로그인 시 자동 시작을 설정하기 전에 물어봅니다(5단계).

<a id="1-detect-the-system"></a>
## 1. 시스템 확인

```sh
uname -s; uname -m
```

| 출력 | 사용 |
|---|---|
| `Linux`에 `x86_64` 또는 `aarch64` | `install.sh`(3단계) |
| `Darwin`에 `arm64` 또는 `x86_64` | `install.sh`(3단계) |
| Windows(`uname`이 없거나 출력에 `MINGW`/`MSYS`가 있음) | PowerShell에서 `install.ps1`(3단계), 또는 WSL에서 `install.sh` |
| 그 밖의 경우 | 소스에서 빌드(3단계) |

<a id="2-look-for-an-existing-install"></a>
## 2. 이미 설치된 것이 있는지 확인

```sh
command -v hyperrouter cli-proxy-api CLIProxyAPI 2>/dev/null
ls -d ~/.hyper-router ~/.cli-proxy-api 2>/dev/null
curl -s -m 2 http://127.0.0.1:8317/healthz; echo " exit=$?"
```

- `~/.hyper-router/config.yaml`이 있으면 hyper-router가 이미 설정된 상태입니다. 계속하기 전에 사용자에게 물어보십시오. 설치 프로그램을 다시 실행해도 실행 파일만 업그레이드하고 서버만 재시작합니다. 설정과 키는 절대 바꾸지 않습니다.
- `cli-proxy-api`나 `CLIProxyAPI`가 있거나, `~/.cli-proxy-api`가 있거나, 8317 포트에 뭔가 응답하면 사용자가 CLIProxyAPI를 쓰고 있을 가능성이 큽니다. 사용자에게 알리고 물어보십시오. 옮길지([CLIProxyAPI에서 옮기기](#migrating-from-cliproxyapi) 참고), 아니면 그 옆에 hyper-router를 설치할지. 설치 프로그램은 기존 설치를 건드리지 않고 옆에 설치합니다. 다음으로 비어 있는 포트와 별도의 인증 폴더를 씁니다.

<a id="3-install-configure-and-start"></a>
## 3. 설치하고 설정하고 켜기

macOS와 Linux에서는 이렇게 합니다.

```sh
curl -fsSL https://raw.githubusercontent.com/alpoxdev/hyper-router/dev/install.sh | sh
```

Windows에서는 PowerShell에서 이렇게 합니다.

```powershell
irm https://raw.githubusercontent.com/alpoxdev/hyper-router/dev/install.ps1 | iex
```

설치 프로그램은 다음을 합니다.

1. 릴리스 실행 파일을 내려받아 릴리스의 체크섬과 대조해 확인하고, `~/.local/bin/hyperrouter`로 설치합니다(Windows에서는 `%LOCALAPPDATA%\Programs\hyper-router\hyperrouter.exe`).
2. `~/.hyper-router/config.yaml`(Windows에서는 `%USERPROFILE%\.hyper-router`)을 씁니다. 8317부터 시작해 비어 있는 포트를 고르고 세션 고정을 켭니다. 클라이언트 키와 관리 키를 만들어 `~/.hyper-router/keys.env`에 저장하고, 이 파일은 사용자만 읽을 수 있게 합니다.
3. 서버를 백그라운드에서 켜고 `~/.hyper-router/hyperrouter.log`에 로그를 남기며 `/healthz`에 응답이 오는지 기다립니다.

대시보드 주소와 파일 위치를 출력합니다. 키는 절대 출력하지 않습니다. 실패하면 이유를 한 줄로 알려 줍니다. `hyperrouter.log`의 마지막 몇 줄을 읽고(키는 들어 있지 않습니다) 알려 주는 문제를 고치십시오.

`keys.env`는 실제 값이 들어 있으면 이렇게 생겼습니다.

```sh
HYPERROUTER_PORT=8317
HYPERROUTER_CLIENT_KEY=sk-...
HYPERROUTER_MANAGEMENT_KEY=...
```

그 시스템에 맞는 릴리스 실행 파일이 없으면 소스에서 빌드합니다. `git`, Rust 도구 모음(`cargo`), `cmake`, `clang`, `perl`이 필요합니다. 설치하기 전에 사용자에게 물어보십시오. 그다음 [설치 스크립트 없이 처음 실행하기](INSTALL.md#first-run-without-the-install-script)를 따라 하고, 위 형식대로 키 두 개를 `~/.hyper-router/keys.env`에 쓰고 `chmod 600`을 실행하십시오.

```sh
git clone https://github.com/alpoxdev/hyper-router.git ~/hyper-router-src
cd ~/hyper-router-src && cargo build --release -p hyperrouter
mkdir -p ~/.local/bin && install -m 0755 target/release/hyperrouter ~/.local/bin/hyperrouter
```

설치 프로그램은 `nohup`으로 서버를 켭니다. 그러면 컴퓨터를 다시 시작할 때까지 서버가 계속 실행됩니다. 자체 서비스 관리자나 프로세스 관리자가 있는 컨테이너나 샌드박스에서는 그 관리자를 쓰십시오. 백그라운드 서버를 `kill $(cat ~/.hyper-router/hyperrouter.pid)`로 멈추고, 관리자 아래에서 `~/.local/bin/hyperrouter --config ~/.hyper-router/config.yaml`을 실행하십시오.

<a id="4-verify"></a>
## 4. 확인

```sh
. ~/.hyper-router/keys.env
curl -fsS "http://127.0.0.1:$HYPERROUTER_PORT/healthz"; echo
curl -s -o /dev/null -w '%{http_code}\n' -H "Authorization: Bearer $HYPERROUTER_CLIENT_KEY" "http://127.0.0.1:$HYPERROUTER_PORT/v1/models"
```

Windows에서는 PowerShell에서 이렇게 합니다.

```powershell
$k = @{}; Get-Content ~\.hyper-router\keys.env | Where-Object { $_ -match '^(\w+)=(.*)$' } | ForEach-Object { $k[$Matches[1]] = $Matches[2] }
Invoke-RestMethod "http://127.0.0.1:$($k.HYPERROUTER_PORT)/healthz"
(Invoke-WebRequest "http://127.0.0.1:$($k.HYPERROUTER_PORT)/v1/models" -Headers @{ Authorization = "Bearer $($k.HYPERROUTER_CLIENT_KEY)" } -UseBasicParsing).StatusCode
```

`{"status":"ok"}`(PowerShell는 `status: ok`로 보여 줍니다)가 나오고 그다음 `200`이 나와야 합니다. `401`이 나오면 `keys.env`의 키와 `config.yaml`의 키가 다르다는 뜻입니다.

<a id="5-start-at-login-only-if-the-user-wants-it"></a>
## 5. 로그인 시 자동 시작(사용자가 원할 때만)

먼저 사용자에게 물어보십시오. 동의하면 `--service`를 붙여 설치 프로그램을 다시 실행합니다.

```sh
curl -fsSL https://raw.githubusercontent.com/alpoxdev/hyper-router/dev/install.sh | sh -s -- --service
```

Windows에서는 `& ([scriptblock]::Create((irm https://raw.githubusercontent.com/alpoxdev/hyper-router/dev/install.ps1))) -Service`입니다. Linux에는 systemd 사용자 유닛을, macOS에는 launchd 에이전트를, Windows에는 로그인 항목을 추가합니다. 자세한 내용과 되돌리는 방법은 [INSTALL.md](INSTALL.md#start-at-login)에 있습니다.

<a id="6-open-the-dashboard"></a>
## 6. 대시보드 열기

사용자가 이 컴퓨터 앞에 앉아 있고 데스크톱이 있다면 이렇게 합니다.

```sh
. ~/.hyper-router/keys.env
URL="http://127.0.0.1:$HYPERROUTER_PORT/management.html"
(open "$URL" || xdg-open "$URL") >/dev/null 2>&1 || echo "Open $URL in a browser"
```

여기서 브라우저를 열 수 없다면(SSH로 접속한 서버, 컨테이너, 원격 샌드박스) 주소를 출력하고, 인터넷에 열지 않고 사용자 컴퓨터에서 그 주소에 닿는 방법을 알려 주십시오.

- 사용자 컴퓨터에서 실행하는 SSH 터널: `ssh -L 8317:127.0.0.1:8317 user@host`(양쪽 모두 `keys.env`의 포트를 쓰십시오), 그런 다음 그 컴퓨터에서 `http://127.0.0.1:8317/management.html`을 엽니다.
- 또는 두 컴퓨터 모두에 [Tailscale](MULTI-ACCOUNT.md#reach-the-proxy-from-other-machines)을 설치합니다.

터널을 쓸 때는 `server.host`를 `127.0.0.1`로 유지하십시오. `access.api-keys`가 비어 있는 동안에는 `server.host`를 `0.0.0.0`이나 `""`로 설정하지 마십시오.

사용자에게 이렇게 알려 주십시오. 로그인에 쓸 관리 키는 `~/.hyper-router/keys.env`의 `HYPERROUTER_MANAGEMENT_KEY` 줄입니다. 사용자는 자기 터미널에서 `cat ~/.hyper-router/keys.env`로 이 값을 볼 수 있습니다.

<a id="7-hand-over-the-sign-in-to-the-user"></a>
## 7. 로그인은 사용자에게 맡기기

사용자에게 계정을 직접 연결해 달라고 말하십시오. 대시보드의 계정 연결에서 하거나 자기 터미널에서 하면 됩니다.

```sh
hyperrouter --config ~/.hyper-router/config.yaml --claude-login
```

(`--codex-login`, `--codex-device-login`, `--kimi-login`, `--meta-login`, `--xai-login`, `--devin-login`도 같은 방식입니다. 브라우저가 없는 컴퓨터에서는 `--no-browser`를 붙이십시오.) 사용자가 다 됐다고 할 때까지 기다립니다. [계정과 제공자 이용약관](../README.md#accounts-and-provider-terms)을 안내하십시오. 이 구성은 한 사람이 자기 계정을 자기 컴퓨터에서 쓰기 위한 것입니다. 제공자가 허용한 연동 방식이나 API 키가 있으면 그것을 쓰십시오.

<a id="8-point-the-users-tools-at-it"></a>
## 8. 사용자의 도구가 이것을 쓰도록 지정하기

대시보드의 도구와 함께 쓰기 페이지에는 주소와 키가 채워진 복사용 설정이 있습니다. 각 도구는 [CLIENTS.md](CLIENTS.md)에서 다룹니다. 사용자 설정을 바꾸기 전에 어떤 도구를 쓰는지 물어보십시오.

Claude Code의 경우 다음은 `~/.claude/settings.json`의 `env` 블록에 프록시를 추가하고, 백업을 남기고, 키를 `keys.env`에서 읽어 대화에 키가 나타나지 않게 합니다.

```sh
set -a; . ~/.hyper-router/keys.env; set +a
python3 - <<'PY'
import json, os, shutil
path = os.path.expanduser("~/.claude/settings.json")
os.makedirs(os.path.dirname(path), exist_ok=True)
data = {}
if os.path.exists(path):
    shutil.copy(path, path + ".bak")
    data = json.load(open(path))
data.setdefault("env", {}).update({
    "ANTHROPIC_BASE_URL": "http://127.0.0.1:" + os.environ["HYPERROUTER_PORT"],
    "ANTHROPIC_AUTH_TOKEN": os.environ["HYPERROUTER_CLIENT_KEY"],
})
json.dump(data, open(path, "w"), indent=2)
print("updated", path)
PY
```

사용자가 로그인한 뒤에는 4단계의 `/v1/models` 요청으로 확인합니다. 이제 목록에 사용자의 모델이 들어 있습니다.

<a id="tips-for-heavier-use"></a>
## 많이 쓸 때의 팁

- Claude나 Codex 계정을 여러 개 쓸 때: 라우팅 방식과 한도는 [MULTI-ACCOUNT.md](MULTI-ACCOUNT.md)를 보십시오.
- Codex 계정: `~/.hyper-router/auth`에 있는 각 Codex 계정 파일에서 `"websockets": true`를 설정하면 더 빠른 WebSocket 전송을 씁니다.
- 세션 고정: 설치 프로그램이 설정하는 `routing.session-affinity: true`를 그대로 두십시오.
- 다른 기기: 인터넷에 포트를 열지 말고 [Tailscale](https://tailscale.com/)을 쓰십시오. [다른 기기에서 프록시에 닿기](MULTI-ACCOUNT.md#reach-the-proxy-from-other-machines)를 보십시오. 클라이언트 키 없이 프록시를 인터넷에 노출하지 마십시오.

<a id="migrating-from-cliproxyapi"></a>
## CLIProxyAPI에서 옮기기

사용자가 요청할 때만 진행합니다.

1. Go 서버의 설정 파일(보통 실행 파일 옆의 `config.yaml`, 또는 서비스 정의에 적힌 경로)과 `auth-dir`(기본값 `~/.cli-proxy-api`)을 찾습니다.
2. 둘을 백업합니다. `cp -a <config.yaml> <config.yaml>.bak`과 `cp -a <auth-dir> <auth-dir>.bak`을 실행합니다.
3. Go 서버를 멈춥니다(서비스나 프로세스). 두 서버가 같은 인증 폴더에서 동시에 실행되면 안 됩니다. 각 서버가 로그인 토큰을 갱신하면서 서로의 토큰을 무효로 만들 수 있기 때문입니다.
4. 같은 파일로 hyper-router를 켭니다. `hyperrouter --config <config.yaml>`입니다. 같은 설정과 계정 파일을 읽고, 기존 관리 키도 계속 동작합니다.
5. 그 설정의 포트와 클라이언트 키로 4단계처럼 확인합니다.

[MIGRATING-FROM-GO.md](MIGRATING-FROM-GO.md)에 무엇이 그대로 넘어오는지와 되돌리는 방법이 정리되어 있습니다.
