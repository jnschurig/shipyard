# Shipyard

A cross-platform GUI launcher for managing multiple installed versions of Ship of Harkinian and other HarbourMasters N64 ports. Downloads GitHub releases, installs them side-by-side, manages a small ROM library, and launches the chosen version.

## Supported games

| Port | Underlying game | macOS | Linux |
|---|---|:---:|:---:|
| Ship of Harkinian | Ocarina of Time (+ Master Quest) | ✓ | ✓ |
| 2 Ship 2 Harkinian | Majora's Mask | ✓ | ✓ |
| Ghostship | Super Mario 64 | ✓ | ✓ |
| Starship | Star Fox 64 (US base + EU/JP voice) | — | ✓ |
| SpaghettiKart | Mario Kart 64 | ✓ (arm64 + intel) | ✓ |

Windows is not supported yet (see `CLAUDE.md` for the rough shape of how to add it).

## Building

Toolchain is pinned via `rust-toolchain.toml` (Rust 1.90, edition 2024).

```sh
cargo run                                       # launch the GUI
cargo test --lib                                # fast unit tests
cargo clippy --all-targets -- -D warnings       # what CI enforces
cargo fmt --all -- --check                      # what CI enforces
```

End-to-end manual install against a real GitHub release:

```sh
cargo run --example real_install -- [--launch] [--mq] <rom_path>
```

### Git hooks

Hooks live in `.pre-commit-config.yaml`. `cargo fmt` and `cargo clippy` run on
`pre-commit`; `cargo test --lib` runs on `pre-push`. Install for every stage the
config uses:

```sh
# prek (https://github.com/j178/prek)
prek install --hook-type pre-commit --hook-type pre-push --overwrite

# pre-commit (https://pre-commit.com)
pre-commit install --hook-type pre-commit --hook-type pre-push --overwrite
```

## Using Shipyard

1. Pick a game from the picker at the top of the window.
2. Click **Install** on a release to download and unpack it. Installs live side-by-side under `<library_root>/<game_slug>/<tag>/`.
3. Click **Import ROM** to add a ROM file to Shipyard's library (`<config_dir>/roms/`). No hashing or validation — Shipyard trusts your pick.
4. Assign an imported ROM to a slot for the selected game.
5. **Launch.** Shipyard symlinks the assigned ROM into the install directory and starts the game detached, so closing Shipyard doesn't kill the game.

GitHub release listings are cached with ETags in `<cache_dir>/etags.json` so cold restarts don't burn the 60/hr anonymous rate limit. Set `GITHUB_TOKEN` if you hit it anyway.

## Where things live

- **Installs:** `<library_root>/<game_slug>/<tag>/` — defaults to your platform's data dir (`~/Library/Application Support/shipyard/versions` on macOS, `~/.local/share/shipyard/versions` on Linux).
- **ROM library:** `<config_dir>/roms/<original_filename>` — plain copies, no transformation.
- **Config:** `<config_dir>/shipyard.json` — stores library root, slot assignments, install overrides, etc.
- **Cache:** `<cache_dir>/etags.json` — GitHub ETag cache.

## Architecture

See `CLAUDE.md` for the architectural notes that take more than one file to understand (per-game pluggable model, slot model, ROM wiring, platform abstraction, install progress, atomic-write conventions, and how to add a new platform like Windows).

Long-form design docs for individual features live under `.local/docs/<feature>/`.
