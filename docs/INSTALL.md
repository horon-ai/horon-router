# HyperRouter 설치하기

> 이 문서에서 알 수 있는 것: hyper-router를 설치하고 처음 실행하는 방법. 설치 스크립트, Homebrew, 릴리스 파일, 소스 빌드, Docker를 모두 다룹니다.

hyper-router는 실행 파일 하나입니다. 이름은 `hyperrouter`이고 Windows에서는 `hyperrouter.exe`입니다. 대시보드가 안에 들어 있습니다. `config.yaml` 파일과 인증 파일을 두는 폴더가 있으면 됩니다. 그 밖에 설치할 것은 없습니다.

<a id="with-the-install-script"></a>
## 설치 스크립트 사용하기

macOS나 Linux에서는 이렇게 합니다.

```sh
curl -fsSL https://raw.githubusercontent.com/alpoxdev/hyper-router/dev/install.sh | sh
```

Windows에서는 PowerShell에서 이렇게 합니다.

```powershell
irm https://raw.githubusercontent.com/alpoxdev/hyper-router/dev/install.ps1 | iex
```

두 스크립트 모두 `sudo`나 관리자 권한, Rust 같은 도구가 필요하지 않습니다. 처음 실행하면 다음 순서로 진행합니다.

1. 내 시스템에 맞는 릴리스 압축 파일을 고르고, 릴리스의 `SHA256SUMS`와 함께 내려받습니다. 검사값이 맞지 않으면 아무것도 설치하지 않고 멈춥니다.
2. 실행 파일을 설치합니다. macOS와 Linux에서는 `~/.local/bin/hyperrouter`, Windows에서는 `%LOCALAPPDATA%\Programs\hyper-router\hyperrouter.exe`입니다.
3. `~/.hyper-router/config.yaml`이 없으면(Windows에서는 `%USERPROFILE%\.hyper-router`) 이 파일을 새로 씁니다. 서버는 `127.0.0.1`에서 8317 포트로 요청을 받습니다. CLIProxyAPI처럼 다른 프로그램이 8317을 이미 쓰고 있으면 그다음으로 비어 있는 포트를 씁니다. 인증 파일을 두는 폴더는 `auth`이고 세션 고정(같은 대화가 같은 계정으로 가도록 묶는 기능)은 켜 둡니다. 클라이언트 키를 만들어 설정 파일 옆의 `keys.env`에 저장합니다. 이 폴더는 나만 읽을 수 있고, 스크립트는 키를 절대 출력하지 않습니다.
4. 서버를 백그라운드에서 켜고 `hyperrouter.log`에 기록합니다. 그다음 `/healthz`가 응답하는지 확인합니다.
5. 대시보드 주소를 출력하고, 브라우저가 있으면 브라우저에서 엽니다.

대시보드는 비밀번호 없이 바로 열립니다. 내 도구는 `HYPERROUTER_CLIENT_KEY`를 씁니다(`cat ~/.hyper-router/keys.env`, Windows에서는 `Get-Content ~\.hyper-router\keys.env`).

같은 스크립트를 다시 실행하면 실행 파일을 최신 릴리스로 올리고 서버를 다시 시작합니다. macOS와 Linux에서는 관리 중인 서버가 실행 중이고 그 마지막 시작 표시가 실행 파일보다 오래되지 않았을 때만 이미 최신인 실행 파일을 그냥 둡니다. `--binary-only`를 쓴 뒤이거나 업그레이드가 중간에 끊긴 뒤에 다시 실행하면, 다시 내려받지 않고 설치된 버전을 시작합니다. Unix 업그레이드는 `hyperrouter.prev`에 하드 링크를 남깁니다. Windows는 실행 중인 실행 파일 이름을 바꾸기 전에 교체본을 준비하고 검사합니다. 두 경우 모두 시작에 실패하면 이전 실행 파일로 되돌리고 응답하는지 확인합니다. 두 스크립트 모두 이미 있는 `config.yaml`이나 `keys.env`를 바꾸지 않습니다. 상태 확인과 출력되는 대시보드 주소는 설정된 host, port, TLS 설정을 따릅니다. 와일드카드 host를 쓰면 루프백 주소를 씁니다. Windows 설치 프로그램은 `--log-file`과 `--working-dir`을 지원하는 릴리스를 요구합니다. 더 오래된 릴리스는 기존 설치를 건드리기 전에 거부합니다.

SSH로 접속하는 서버처럼 브라우저가 없는 컴퓨터에서는 서버를 `127.0.0.1`에 두고 내 컴퓨터에서 SSH 터널을 여십시오. 예를 들어 `ssh -L 8317:127.0.0.1:8317 you@server`를 실행한 뒤 그 컴퓨터에서 `http://127.0.0.1:8317/management.html`을 엽니다. [Tailscale](MULTI-ACCOUNT.md#reach-the-proxy-from-other-machines)을 써도 됩니다. 클라이언트 키 없이 `0.0.0.0`에서 요청을 받으면 안 됩니다.

원한다면 실행하기 전에 [install.sh](../install.sh)나 [install.ps1](../install.ps1)을 직접 읽어 보십시오.

<a id="options"></a>
### 옵션

| install.sh | install.ps1 | 효과 |
| --- | --- | --- |
| `--service` | `-Service` | 로그인할 때 hyper-router도 함께 시작합니다. 아래를 보십시오. |
| `--binary-only` | `-BinaryOnly` | 실행 파일만 설치하거나 업그레이드하고 거기서 멈춥니다. |
| `--check` | - | 실행 파일을 내려받거나 파일을 바꾸거나 다시 시작하지 않고, 설치된 버전과 쓸 수 있는 버전을 알려 줍니다. |

파이프로 넘긴 스크립트에 옵션을 주려면 이렇게 합니다.

```sh
curl -fsSL https://raw.githubusercontent.com/alpoxdev/hyper-router/dev/install.sh | sh -s -- --service
```

```powershell
& ([scriptblock]::Create((irm https://raw.githubusercontent.com/alpoxdev/hyper-router/dev/install.ps1))) -Service
```

두 스크립트는 다음 환경 변수를 읽습니다.

| 변수 | 기본값 | 뜻 |
| --- | --- | --- |
| `HYPERROUTER_VERSION` | 최신 릴리스 | 설치할 릴리스 태그입니다. 예를 들어 `v0.1.0`입니다. |
| `HYPERROUTER_INSTALL_DIR` | `~/.local/bin`, Windows에서는 `%LOCALAPPDATA%\Programs\hyper-router` | 실행 파일을 두는 위치입니다. |
| `HYPERROUTER_HOME` | `~/.hyper-router`, Windows에서는 `%USERPROFILE%\.hyper-router` | 설정, 키, 인증 파일, 로그가 들어 있는 폴더입니다. |
| `HYPERROUTER_NO_OPEN` | 설정하지 않음 | `1`로 두면 브라우저를 절대 열지 않습니다. |
| `HYPERROUTER_RELEASES` | `https://github.com/alpoxdev/hyper-router/releases` | 미러나 설치 프로그램 시험용 릴리스 페이지 기준 주소입니다. `HYPERROUTER_VERSION`을 설정하지 않으면 `<base>/latest`에 보낸 HEAD 요청이 `/tag/<tag>`로 끝나는 최종 주소로 넘어가야 합니다. 설치 프로그램은 그 주소에서 태그를 읽습니다. 이 서버는 `/download/<tag>/SHA256SUMS`와 `/download/<tag>/` 아래의 플랫폼 압축 파일도 제공해야 합니다. |

믿을 수 있는 미러만 쓰십시오. 설치 프로그램은 그 미러가 제공하는 `SHA256SUMS` 파일과 압축 파일을 대조합니다.

<a id="start-at-login"></a>
### 로그인 시 시작하기

`--service` 없이 실행하면 서버는 컴퓨터를 다시 시작할 때까지(Windows에서는 로그아웃할 때까지) 실행됩니다. `--service`를 주면 이렇게 됩니다.

- Linux에서는 systemd 사용자 유닛 `~/.config/systemd/user/hyperrouter.service`를 만들고 활성화해 시작합니다. 로그인한 동안 실행됩니다. 로그인 세션 없이 돌아가야 하는 서버라면 `loginctl enable-linger`를 한 번 실행하십시오. 멈추려면 `systemctl --user disable --now hyperrouter`를 실행합니다.
- macOS에서는 launchd 에이전트 `~/Library/LaunchAgents/io.github.vayungodara.hyper-router.plist`를 만듭니다. 서버가 멈추면 다시 켜 주기도 합니다. 멈추려면 `launchctl bootout gui/$(id -u)/io.github.vayungodara.hyper-router`를 실행하고 그 파일을 지웁니다.
- Windows에서는 `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` 아래에 `hyper-router` 항목을 만듭니다. 이 항목은 로그인할 때 `hyperrouter.exe --config ... --log-file ... --working-dir ...`를 직접 실행합니다. 서버가 떨어져 나가기 전에 콘솔이나 Windows Terminal 창이 잠깐 보일 수 있습니다. 작업 폴더는 로그인할 때도 내 설정 폴더입니다. 그래서 `.env`와 상대 경로가 설치한 뒤와 똑같이 해석됩니다. 남아 있는 PowerShell, WMI, cmd.exe 실행기는 없습니다. 프로세스 로그는 10 MiB마다 교체됩니다. `logs-max-total-size-mb`를 양수로 두면 그 예산을 일반 로그 폴더와 함께 씁니다. 그 폴더 바깥의 교체본도 포함합니다. 기존 정리 작업이 1분마다 돕니다. 기본값 `0`이면 일반 로그는 제한 없이 커집니다. 다만 `--log-file` 교체본에는 별도의 총 32 MiB 상한이 있고, 이 상한은 별도 타이머 없이 시작할 때와 교체할 때 적용됩니다. 두 정리 경로 모두 현재 쓰는 출력 파일은 지우지 않습니다. 그래서 그 파일은 예산을 넘을 수 있습니다. `logging-to-file: true`이면 대시보드용 `main.log`는 그대로 씁니다. 이 항목은 `Remove-ItemProperty HKCU:\Software\Microsoft\Windows\CurrentVersion\Run -Name hyper-router`로 지웁니다.

한 번 설정하면 이후 업그레이드도 같은 방식으로 서버를 다시 시작합니다. 시스템 페이지는 클릭할 때만 새 릴리스가 있는지 확인합니다. 이때 익명 HEAD 요청을 보내고 12시간 동안 그 결과를 저장합니다. 이 확인을 끄려면 서버를 시작하기 전에 서버 환경에 `HYPERROUTER_NO_UPDATE_CHECK=1`을 설정하십시오. 릴리스 확인은 설정한 `proxy-url`을 쓰고, 설정하지 않았으면 환경 프록시를 씁니다. 토큰이나 쿠키는 절대 보내지 않습니다. 프록시 인증 정보는 내 프록시에만 전달되고, TLS 검증은 켜 두고 리디렉션은 끕니다.

<a id="removing-it"></a>
### 삭제하기

서버를 멈추고(`kill $(cat ~/.hyper-router/hyperrouter.pid)` 또는 위에 나온 Unix 서비스 명령) 실행 파일을 지웁니다. Windows에서는 설정 파일 경로로 프로세스를 골라야 합니다. 로그인 시 직접 실행한 프로세스는 설치 프로그램의 pid 파일을 갱신하지 않기 때문입니다.

```powershell
$config = Join-Path $env:USERPROFILE '.hyper-router\config.yaml'
Get-CimInstance Win32_Process -Filter "Name LIKE 'hyperrouter%.exe'" |
  Where-Object { $_.CommandLine -and $_.CommandLine.Contains($config) } |
  ForEach-Object { Stop-Process -Id $_.ProcessId }
```

`HYPERROUTER_HOME`을 설정했다면 실제 설정 경로를 쓰십시오. 이 명령은 실행 파일 폴더를 함께 쓰는 다른 설정은 건드리지 않고 실행 상태로 둡니다. `~/.hyper-router`에는 내 설정, 키, 로그인한 계정이 들어 있습니다. 더 필요 없을 때만 지우십시오.

<a id="homebrew"></a>
## Homebrew

macOS나 Linux에서는 tap에서 설치합니다.

```sh
brew install vayungodara/tap/hyper-router
brew services start hyper-router
```

이 formula는 Apple silicon, Intel macOS, Linux arm64, Linux x86_64용 릴리스 실행 파일을 내려받습니다. Rust로 컴파일하지는 않습니다. Linux는 릴리스 압축 파일과 마찬가지로 glibc 2.35 이상이 필요합니다. 오래된 Linux에서는 Homebrew가 자체 실행 의존 패키지를 함께 설치할 수 있습니다. 이것은 hyperrouter 실행 파일과 별개입니다.

처음 설치하면 `$(brew --prefix)/etc/hyper-router/config.yaml`을 만듭니다. 이 설정은 `127.0.0.1:8317`에서만 요청을 받습니다. 만들어 낸 클라이언트 키와 관리 키는 그 옆의 `keys.env`에 넣습니다. 이 폴더는 사설 폴더이고 설정과 키는 설치한 사용자만 읽을 수 있습니다. 로그인 인증 정보는 그 아래 `auth` 폴더에 들어갑니다. 업그레이드는 이미 있는 설정이나 키를 절대 바꾸지 않습니다. 설정이 없어졌지만 `keys.env`가 남아 있으면 formula는 그 키를 다시 씁니다.

`http://127.0.0.1:8317/management.html`을 엽니다(비밀번호 없이 바로 열립니다). 내 도구는 `HYPERROUTER_CLIENT_KEY`를 씁니다. 다른 서버가 8317 포트를 쓰면 서비스를 시작하기 전에 설정에서 `server.port`를 바꾸십시오. 서비스는 `sudo` 없이 내 사용자로 실행하십시오.

이 서비스는 설정 경로를 직접 넘기므로 현재 폴더에 기대지 않습니다. 로그는 `$(brew --prefix)/var/log/hyper-router.log`에 남습니다. 멈추려면 `brew services stop hyper-router`, 실행 파일을 지우려면 `brew uninstall hyper-router`를 실행합니다. 내 설정, 키, 로그인 인증 정보는 직접 지울 때까지 남습니다.

유지보수자용 안내입니다. 이 저장소의 Actions secrets에 `HOMEBREW_TAP_TOKEN`을 추가하십시오. 그 토큰에는 tap에 대한 contents 쓰기 권한만 주십시오. 정식 릴리스를 게시하면 릴리스의 `SHA256SUMS`로 `Formula/hyper-router.rb`를 만들고 tap에 커밋합니다. 토큰이 없으면 이 작업은 갱신을 건너뜁니다. 사전 릴리스는 formula를 갱신하지 않습니다. 게시해도 릴리스 빌드는 다시 돌지 않습니다. 원본 template은 [`packaging/homebrew/hyper-router.rb`](../packaging/homebrew/hyper-router.rb)에 있습니다.

<a id="from-a-release"></a>
## 릴리스 파일로 설치하기

[릴리스](https://github.com/alpoxdev/hyper-router/releases)마다 플랫폼별 압축 파일과 `SHA256SUMS` 파일이 있습니다.

| 플랫폼 | 압축 파일 |
| --- | --- |
| Linux x86_64 | `hyperrouter-<version>-x86_64-unknown-linux-gnu.tar.gz` |
| Linux arm64 | `hyperrouter-<version>-aarch64-unknown-linux-gnu.tar.gz` |
| macOS Apple silicon | `hyperrouter-<version>-aarch64-apple-darwin.tar.gz` |
| macOS Intel | `hyperrouter-<version>-x86_64-apple-darwin.tar.gz` |
| Windows x86_64 | `hyperrouter-<version>-x86_64-pc-windows-msvc.zip` |

릴리스에는 `management.html`도 들어 있습니다. Go로 만든 CLIProxyAPI 서버용 대시보드를 파일 하나로 묶은 것입니다([ui/PANEL.md](../ui/PANEL.md) 참고). Rust 실행 파일에는 필요하지 않습니다.

실행하기 전에 내려받은 파일을 확인합니다.

```sh
sha256sum --check --ignore-missing SHA256SUMS      # Linux
shasum -a 256 --check --ignore-missing SHA256SUMS  # macOS
```

Windows에서는 `Get-FileHash hyperrouter-<version>-x86_64-pc-windows-msvc.zip`의 출력을 `SHA256SUMS`의 해당 줄과 비교합니다.

압축을 풀고 실행 파일을 `PATH`에 넣습니다.

```sh
tar -xzf hyperrouter-<version>-x86_64-unknown-linux-gnu.tar.gz
sudo install -m 0755 hyperrouter-<version>-x86_64-unknown-linux-gnu/hyperrouter /usr/local/bin/hyperrouter
hyperrouter --version
```

macOS 실행 파일에는 서명이나 공증이 없습니다. Gatekeeper가 처음 실행을 막으면 `xattr -d com.apple.quarantine /usr/local/bin/hyperrouter`로 격리 표시를 지우십시오.

Linux 실행 파일은 glibc와 libstdc++에 연결되어 있고 최신 배포판(Debian 12, Ubuntu 22.04 이상, Fedora, Arch)에서 실행됩니다. Alpine처럼 musl을 쓰는 시스템에서는 소스에서 빌드하거나 Docker 이미지를 쓰십시오.

<a id="from-source"></a>
## 소스에서 빌드하기

기여자와, Alpine처럼 릴리스 실행 파일이 없는 시스템을 위한 방법입니다. Rust 도구 모음(stable, 1.88 이상)과 BoringSSL을 빌드할 도구가 필요합니다. `cmake`, `clang`, `perl`입니다. Debian이나 Ubuntu에서는 이렇게 합니다.

```sh
sudo apt-get install -y build-essential cmake clang perl git
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

macOS에서는 `xcode-select --install`과 `brew install cmake`로 충분합니다. Windows에서는 C++ 작업이 포함된 Visual Studio Build Tools, CMake, LLVM(bindgen에 `libclang`이 필요합니다), NASM(BoringSSL의 x86_64 어셈블리에 필요합니다)을 설치하십시오. 예를 들어 `choco install cmake llvm nasm`을 쓰고 NASM을 `PATH`에 넣습니다. Windows 빌드는 릴리스 워크플로가 만들며, 손으로 시험해 본 적은 없습니다.

그다음 빌드합니다.

```sh
git clone https://github.com/alpoxdev/hyper-router.git
cd hyper-router
cargo build --release -p hyperrouter
./target/release/hyperrouter --version
```

WebRTC 미디어 릴레이는 선택 기능이고 릴리스 실행 파일이나 Docker 이미지에 들어 있지 않습니다. 포함하려면 `cargo build --release -p hyperrouter --features hr-server/media-relay`로 빌드하십시오.

`ui/dist`의 대시보드는 저장소에 들어 있으므로 보통 빌드에는 Node가 필요하지 않습니다. 대시보드를 바꾸려면 [ui/README.md](../ui/README.md)를 보고 실행 파일을 빌드하기 전에 UI를 다시 빌드하십시오.

<a id="docker"></a>
## Docker

저장소의 `Dockerfile`은 실행 파일을 빌드해 권한 없는 사용자로 실행되는 작은 Debian 이미지에 넣습니다. 빌드 중에 BoringSSL을 컴파일하므로 메모리가 약 2 GB 필요합니다. 작은 컴퓨터에서는 `--build-arg BUILD_JOBS=1`을 붙이십시오.

```sh
docker build -t hyper-router .
mkdir -p data/auth
$EDITOR data/config.yaml                  # 아래 설명을 보십시오
sudo chown -R 10001:10001 data            # 컨테이너는 uid와 gid 10001로 실행됩니다
docker run -d --name hyperrouter -p 127.0.0.1:8317:8317 -v "$PWD/data:/data" hyper-router
```

컨테이너 안에서 설정 파일은 `/data/config.yaml`입니다. 그 설정에서 다음을 바꿉니다.

- 서버가 컨테이너의 인터페이스에서 요청을 받도록 `server.host`를 `""`나 `0.0.0.0`으로 설정합니다.
- 인증 정보가 마운트한 볼륨에 남도록 `oauth.auth-dir`를 `/data/auth`로 설정합니다.
- 대시보드를 쓰려면 `management.allow-remote: true`로 설정합니다. Docker는 게시한 포트를 브리지 네트워크에서 전달합니다. 그래서 내 컴퓨터에서 보낸 요청도 로컬이 아닌 주소에서 온 것처럼 도착합니다. `allow-remote: false`이면 관리 API가 그 요청을 거부합니다("remote management disabled"). 위처럼 `127.0.0.1`에 포트를 게시하면 다른 컴퓨터는 여전히 들어올 수 없습니다. Go 서버도 컨테이너 안에서 똑같이 동작합니다.

서버는 `data/`에 쓸 수 있어야 합니다. 처음 시작할 때 관리 키를 해시해서 `config.yaml`에 저장하고, 대시보드는 설정과 인증 파일을 씁니다. 쓸 수 없으면 키가 평문으로 남고 저장이 실패합니다. 위의 `chown`이 이 문제를 막습니다. 다른 컴퓨터가 접속할 필요가 없으면 `127.0.0.1`에 포트를 게시하십시오. [안전하게 실행하기](GETTING-STARTED.md#running-it-safely)를 보십시오.

컨테이너 안에서 브라우저로 로그인하려면 OAuth 콜백 포트(Claude는 54545, Codex는 1455)를 내 컴퓨터로 게시해야 합니다. 그래서 보통은 대시보드에서 로그인하고 마지막 콜백 주소를 붙여 넣는 편이 쉽습니다. 또는 기기 코드를 쓰는 `--codex-device-login`, Kimi, Meta를 쓰십시오.

```sh
docker exec -it hyperrouter hyperrouter --config /data/config.yaml --codex-device-login
```

<a id="first-run-without-the-install-script"></a>
## 설치 스크립트 없이 처음 실행하기

실행 파일을 다른 방법으로 설치했다면 손으로 설정합니다.

1. 예를 들어 `openssl rand -hex 24`로 무작위 클라이언트 키를 만들고 `config.yaml`을 씁니다.

   ```yaml
   server:
     host: "127.0.0.1"          # 이 컴퓨터에서만 접속할 수 있습니다
     port: 8317
   access:
     api-keys:
       - "PASTE-CLIENT-KEY"     # 내 도구가 프록시로 보내는 키입니다
   # management:
   #   secret-key: "PASTE-MANAGEMENT-KEY"  # 선택: 대시보드 비밀번호. 없으면 대시보드가 바로 열립니다
   oauth:
     auth-dir: "~/.hyper-router/auth"     # 계정 로그인 정보를 저장하는 곳입니다
   ```

   관리 키를 설정했다면 사본을 남겨 두십시오. 처음 시작할 때 서버가 `config.yaml`에 있는 그 값을 bcrypt 해시로 바꿉니다. 관리 키가 없으면 대시보드와 관리 API가 인증 없이 열립니다(Go CLIProxyAPI는 이때 관리 API를 끕니다). CLIProxyAPI의 [`config.example.yaml`](https://github.com/router-for-me/CLIProxyAPI/blob/main/config.example.yaml)에 있는 모든 설정을 받습니다. 다만 아직 지원하지 않는다고 표시된 기능의 설정은 효과가 없습니다. 자주 쓰는 설정은 [CONFIGURATION.md](CONFIGURATION.md)가 설명합니다.
2. 서버를 시작합니다. `hyperrouter --config config.yaml`을 실행합니다. `--config`를 주지 않으면 현재 폴더의 `config.yaml`을 읽습니다. `curl http://127.0.0.1:8317/healthz`로 확인하면 `{"status":"ok"}`를 돌려줍니다.
3. `--claude-login`, `--codex-login`, `--codex-device-login`, `--kimi-login`, `--kimi-ai-login`, `--meta-login`, `--xai-login`, `--devin-login`으로 계정을 연결합니다. `--vertex-import key.json`으로 Vertex AI 서비스 계정을 가져오거나, `/management.html`의 대시보드에서 계정을 연결합니다. 브라우저가 없는 컴퓨터에서는 `--no-browser`를 붙이십시오.

`-config config.yaml`처럼 Go 방식의 한 개 대시 플래그도 됩니다.

서버는 `config.yaml`과 인증 파일 폴더가 바뀌면 다시 읽습니다. 로그를 더 자세히 보려면 `RUST_LOG=debug`를 설정하십시오. 기본 단계는 `info`입니다.

<a id="running-as-a-system-service"></a>
## 시스템 서비스로 실행하기

사용자별 서비스를 쓰려면 설치 스크립트의 `--service`를 쓰십시오. Linux 서버라면 아래 같은 system 유닛으로 전용 사용자 아래에서 실행합니다.

```ini
# /etc/systemd/system/hyperrouter.service
[Unit]
Description=hyper-router
After=network-online.target
Wants=network-online.target

[Service]
User=hyperrouter
WorkingDirectory=/var/lib/hyperrouter
ExecStart=/usr/local/bin/hyperrouter --config /var/lib/hyperrouter/config.yaml
Restart=on-failure
NoNewPrivileges=true
ProtectSystem=strict
ReadWritePaths=/var/lib/hyperrouter

[Install]
WantedBy=multi-user.target
```

```sh
sudo useradd --system --home /var/lib/hyperrouter --create-home hyperrouter
sudo systemctl enable --now hyperrouter
journalctl -u hyperrouter -f
```

서버는 `config.yaml`에(대시보드가 설정을 저장하거나 평문 관리 키를 해시할 때) 그리고 인증 파일 폴더에 씁니다. 그래서 둘 다 서비스 사용자가 쓸 수 있어야 합니다.

Windows에서 로그인한 사용자 없이 시스템 서비스로 실행하려면 NSSM 같은 서비스 관리자로 감싸십시오.
