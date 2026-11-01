# Contributing

Thanks for considering a contribution. The lowest-friction path is **a new scaffold**: pick an app you use, write a TOML manifest, open a PR.

## Dev environment

- Rust 1.80+ (`rustup default stable`)
- Node 20+, pnpm 9+
- Tauri prerequisites: <https://tauri.app/start/prerequisites/>
- On Windows, install the WebView2 runtime if missing.

```bash
pnpm install
pnpm tauri dev          # run desktop app
cargo test --workspace  # test Rust crates
pnpm lint               # eslint + prettier check
pnpm check:rust         # cargo fmt --check + clippy -D warnings（同 CI lint-rust）
```

### Windows long-path note

If the checkout lives on a deep path (e.g. `J:\...\diskpilot_src`), a cold build of the `aws-lc-sys` dependency can fail inside MSVC `lib.exe` with exit code 1104 — the link command line exceeds the 32k limit because of hundreds of long `.o` paths under `target\`. This is unrelated to your change. Workaround:

```bash
# use a short target dir (e.g. C:\t) and skip admin-only tests
export CARGO_TARGET_DIR=/c/t
export DISKPILOT_NO_ADMIN=1
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

## Branching & commits

- Branch from `main`, named `feat/...`, `fix/...`, `scaffold/<app>`.
- Sign off your commits (`git commit -s`); we use [DCO](https://developercertificate.org/) instead of a CLA.
- Keep PRs focused. New scaffolds = one PR per app.

## Writing a scaffold

See [docs/scaffold-authoring.md](../docs/scaffold-authoring.md). The PR template asks for:

- detection paths
- risk classification (`low` / `medium` / `high`)
- a screenshot of the scaffold matching on a real machine
- explicit confirmation that no message database / private data is touched

## Code review

- Small PRs are reviewed within ~48h.
- We squash-merge.
- CI must be green.

## Reporting issues

Please use the issue templates. For security issues see [SECURITY.md](SECURITY.md) – do **not** open a public issue.
