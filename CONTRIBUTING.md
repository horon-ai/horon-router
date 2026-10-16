# Contributing

Thanks for helping. Bug reports, fixes and new tests are all welcome.

## Before you start

- horon-router follows CLIProxyAPI at commit `6fecc6e`. When the two disagree, CLIProxyAPI's code is the reference, unless the difference is listed in [docs/DIFFERENCES-FROM-GO.md](docs/DIFFERENCES-FROM-GO.md). [docs/PARITY.md](docs/PARITY.md) shows what is ported and what is not.
- For a larger change, open an issue first so we can agree on the approach.

## Building and testing

You need Rust (stable), `cmake`, `clang` and `perl` for BoringSSL. PostgreSQL lets the storage tests run instead of skipping; with `CPA_TEST_NO_SKIP=1`, as in CI, those tests fail instead. The Go differential harness in `harness/` is separate from `cargo test` and needs Go 1.26 on Linux.

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

Tests use local mock upstreams only. Never point a test at a real provider or use a real account or key, and run the tests with external network access blocked if you can; CI runs them in a network namespace with only a loopback interface. The BoringSSL build and the release link use a lot of memory, so on a small machine set `CARGO_BUILD_JOBS=2`.

### Performance budgets

CI holds the binary to the numbers in [`bench/budgets.txt`](bench/budgets.txt), and it counts events and bytes, never elapsed time:

- Size, on every pull request: the Linux release binary, its `.text` and `.rodata`, the release archive, and the number of crates linked in more than one version.
- Idle, on every pull request: [`bench/idle.sh`](bench/idle.sh) with 1 and with 2,000 auth files: wakeups, CPU ticks, threads and RSS over 30 seconds without requests.
- Heap per request: `crates/horon-server/tests/alloc_budget.rs`, part of `cargo test`, sends one 306 KB and one 1.9 MB request through the Claude route, the same sizes through Claude count_tokens (counted locally) and through the Codex Responses route, under a counting allocator.
- macOS and Windows idle, in the release builds and weekly ([`idle.yml`](.github/workflows/idle.yml)). Memory soaks ([`soak.yml`](.github/workflows/soak.yml), [`bench/soak.sh`](bench/soak.sh)): one hour of the Claude load and one hour of the field mix (Claude, Codex and count_tokens requests of 200 KB to 2 MB) on every release tag, 300 minutes of the field mix every week, and up to 330 minutes by hand before a release. The resting RSS must stay flat, and the field mix's peak (`VmHWM`) within its budget.

To run the Linux gates locally, build with `cargo build --release -p horon-router`, then run `bench/gate.sh size x86_64-unknown-linux-gnu target/release/horonrouter` and `bench/gate.sh idle linux target/release/horonrouter 1`. When a change needs more, raise the value in `bench/budgets.txt` and add a dated line above it that names the feature and its cost.

The dashboard lives in `ui/` (React 19, Tailwind CSS, and shadcn-style Base UI components). Run `npm ci`, `npm run check`, `npm test` and `npm run build` there; the build also prints the gzip size of the JavaScript and CSS. See [ui/README.md](ui/README.md).

## Pull requests

- Keep each pull request to one change, with a test that fails without it.
- Request bodies are forwarded byte for byte unless a ported rule rewrites them; do not re-serialize JSON just to pass it through.
- Never log or forward credentials or client keys to the wrong side.
- Mark a deliberate simplification with a `ponytail:` comment that names its limit and how to lift it.
- CI must pass: format, clippy, tests, the harness check, clippy on Windows (all targets, warnings as errors) and the Linux size and idle gates.

By contributing you agree that your work is released under the [MIT license](LICENSE).
