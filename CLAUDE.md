# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

Shipyard is a cross-platform GUI launcher for managing multiple installed versions of Ship of Harkinian (SoH) and related HarbourMasters projects. It downloads GitHub releases, installs them side-by-side, manages a small ROM library, and launches the chosen version. Single Rust binary; iced for UI; tokio + reqwest for async I/O.

The crate exposes both `bin` (`src/main.rs`) and `lib` (`src/lib.rs`) — most logic lives in the lib so tests and `examples/real_install.rs` can drive it.

## Common commands

- `cargo run` — launch the GUI
- `cargo test --lib` — fast unit tests
- `cargo test --all` — lib + examples + doc tests
- `cargo test --lib <pattern>` — run a subset (e.g. `cargo test --lib roms::wiring`)
- `cargo clippy --all-targets -- -D warnings` — what CI enforces
- `cargo fmt --all -- --check` — what CI enforces
- `cargo run --example real_install -- [--launch] [--mq] <rom_path>` — end-to-end manual validation against a real SoH GitHub release

Toolchain pinned in `rust-toolchain.toml` (1.90.0, edition 2024).

## Architecture (the parts you have to read multiple files to understand)

**Filesystem is the source of truth for installs, not config.** Each install dir carries a `.shipyard-install.json` manifest. `library::scan` discovers installs by walking `library_root` + any `Config::install_overrides`. Config never stores an `installs` list. There is no separate "adopt" verb — having a manifest *is* adoption.

**Per-game pluggable model.** Everything game-specific is behind the `games::Game` trait (`src/games/mod.rs`). Five implementations ship today:

- **SoH** (`src/games/soh.rs`) — Ocarina of Time. Dual OoT / OoT-MQ slots. macOS: `.app`-in-DMG-in-zip. Linux: flat zip with `soh.appimage` at root.
- **2Ship** (`src/games/twoship.rs`) — Majora's Mask. Single slot. macOS: `.app`-in-DMG-in-zip. Linux: flat zip with `2ship.appimage`.
- **Ghostship** (`src/games/ghostship.rs`) — SM64. Single slot. macOS: `.app`-in-DMG-in-zip. Linux: flat zip with `ghostship.appimage`.
- **Starship** (`src/games/starship.rs`) — Star Fox 64. Three slots (US base + EU/JP voice replacements; only US has a cached asset since EU/JP behavior is unverified). Linux only — upstream Linux zip ships an `assets/yaml/...` tree the game reads at runtime, so the install dir gets the full zip extracted, not just the appimage.
- **SpaghettiKart** (`src/games/spaghettikart.rs`) — Mario Kart 64. Single slot. Linux: flat zip with `spaghetti.appimage`. macOS: flat zip (no `.app`, no DMG) with `Spaghettify` binary at root, separate arm64 / intel-x64 builds selected via `std::env::consts::ARCH`.

`games::registry()` enumerates them; the UI exposes a runtime game picker via `App.selected_game_slug`, and installs are partitioned on disk by game slug (`<library_root>/<game_slug>/<tag>/`). When adding a game-specific behavior, the answer is almost always "add it to the `Game` trait", not "branch on game slug".

**Slot identifiers are per-game `&'static str`.** `SlotSpec::id` and `CachedAssetSpec::slot_id` share the same identifier space within a game. `Config.slot_assignments` is `HashMap<game_slug, HashMap<slot_id, rom_filename>>`. Do not introduce a global enum of slots.

**ROM library + launch wiring.** Imported ROMs are plain copies in `<config_dir>/roms/<original_filename>` (no hashing, no validation, no format detection — trust the user's pick). Slot assignments map a slot to a ROM filename. At launch, `roms::wiring::reconcile` places `<install_dir>/<slot.symlink_filename>` as a symlink to the assigned ROM in the library — **unconditionally**, every launch, for every assigned slot. The ROM symlink must be in place whenever the game decides to regenerate its cached archive, and detecting that condition reliably from the launcher is brittle; the unconditional reconcile is cheap (a stat + maybe a symlink rename) and removes the failure mode entirely. Symlinks are placed atomically (symlink → rename) and skipped only when the existing link already points at the right target. Launch passes no ROM-related CLI args; the symlink is the only wiring mechanism. There is also an opt-in real-file-copy path (`Game::requires_rom_copy()` → `true`, atomic copy → rename, skip-if-same-size in `place_copy`) for any future port whose loader rejects symlinks; no shipping game uses it today, but it's wired and tested.

**Config migration is split into two phases.** `Config::load_from` is pure: it returns a `LoadedConfig` whose `pending_migration: Option<PendingMigration>` describes filesystem work that needs to happen (ROM imports). `main.rs` (and tests) materialize the migration via `roms::library::apply_pending_migration`, then `Config::save_to` to persist. Don't add filesystem side effects to `Config::load_from`.

**Platform abstraction is OS-only.** `platform::Platform` covers `default_library_root`, `config_dir`, `cache_dir`, `asset_keyword` — that's it. Asset selection (`pick_asset`), launch commands (`launch_command`), and extraction (`extract`) belong on `Game`, not `Platform`. The two platform modules also export named install primitives that games dispatch into:

- `platform::linux::install_appimage_release(archive, dest, appimage_name)` — every Linux release Shipyard supports is a flat zip with the appimage at root alongside data files (`gamecontrollerdb.txt`, asset trees, `config.yml`). Unzips the whole archive into `dest`. Extracting only the appimage strips runtime resources and crashes the game (Starship's `AudioLoad_Init` SIGSEGVs).
- `platform::macos::install_app_in_dmg_release(archive, dest)` — `.app`-in-DMG-in-zip pattern (SoH/2Ship/Ghostship). Mounts via `hdiutil` with a RAII detach guard (auto-accepts SLA prompts by piping `Y\n` to stdin — Ghostship's DMG ships with one), copies the `.app` to dest. macOS-only body, Linux stub returns an error so the game extract dispatchers stay platform-agnostic at the call site.
- `platform::macos::install_flat_binary_release(archive, dest, binary_name)` — flat zip with a non-`.app` binary at root (SpaghettiKart pattern).

Both flat-zip wrappers delegate to a private `library::extract::install_flat_zip` helper, so they can't drift. A game's `extract` is a 4-line dispatch:

```rust
fn extract(&self, archive: &Path, dest: &Path, platform: &dyn Platform) -> Result<()> {
    match platform.asset_keyword() {
        "Mac" => macos::install_app_in_dmg_release(archive, dest),
        "Linux" => linux::install_appimage_release(archive, dest, "soh.appimage"),
        other => Err(anyhow!("SoH: unsupported platform keyword {other}")),
    }
}
```

**Adding Windows support.** Roughly:
1. Create `src/platform/windows.rs` with a `Windows` struct implementing `Platform` (likely `asset_keyword() = "Win64"` to match HarbourMasters' release naming convention).
2. Add OS-specific install primitives next to it — at minimum `windows::install_zip_release(archive, dest, exe_name)` for the common flat-zip-with-.exe shape. Some games may need an installer (`.msi`, `.exe`-installer) variant; add primitives as patterns appear, don't pre-build.
3. Each game's `extract` gains a `"Win64" =>` arm calling the appropriate primitive. Some games may not ship Windows builds — leave them returning `Err(anyhow!(...))` like Starship currently does for macOS.
4. Each game's `pick_asset` and `launch_command` already match on `asset_keyword`, so they need a `"Win64"` arm too. Conventions like `.exe` extensions and CWD handling differ from Unix and should be encoded per-game.
5. Cross-cutting: `roms::wiring` uses Unix symlinks (Windows symlink semantics differ — admin/dev-mode required for true symlinks); the `requires_rom_copy()` escape hatch on `Game` can flip Windows to file-copy mode without changing the symlink path. The `setsid()` launcher detach is `#[cfg(unix)]` and needs a Windows equivalent (`CREATE_NEW_PROCESS_GROUP` flag, or just letting Windows handle it since closing the parent doesn't kill children by default).

**GitHub client persists ETags** to `<cache_dir>/etags.json` so cold restarts don't burn the 60/hr anonymous rate limit. `GITHUB_TOKEN` env var is the user-facing escape hatch.

**Install progress.** `library` emits an `InstallProgress` enum (`Downloading { downloaded, total }`, `Extracting`, `Finalizing`) over a channel during installs. The UI maps it to `App.install_progress: HashMap<tag, Option<u8>>` via `Message::InstallProgress(tag, percent)` and renders a per-tag progress bar. When wiring new long-running install steps, push status through this enum rather than adding a parallel signaling path.

**Extraction helpers.** `library::extract` exposes `unzip` (preserves Unix permissions from the zip), `install_flat_zip` (`pub(crate)` shared by the platform install wrappers — don't call directly from games), and on macOS `find_first_with_ext` + `mount_dmg` + `copy_dir_recursive` (used by `platform::macos::install_app_in_dmg_release`; gated to `#[cfg(target_os = "macos")]` so Linux builds don't carry dead code or unused-import warnings).

**Atomic writes everywhere.** Configs, manifests, installs (`<dest>.partial` → rename), ROM imports, symlinks (`symlink → rename`). When adding a write, follow the same pattern.

## Conventions worth knowing

- Banner-driven error UX: surface failures by pushing onto `App.banners`, not modal dialogs (modals are reserved for destructive confirms like clear-cache).
- Tests prefer real filesystems via `tempfile::tempdir()` and real subprocesses (`/usr/bin/true`, shell shims) over mocks. `wiremock` is used for HTTP only.
- The launcher detaches via `setsid()` (Unix `pre_exec`) so closing Shipyard doesn't kill running games.
- Long-form design docs live in `.local/docs/<feature>/{01-requirements,02-plan}.md` (the `/spec-and-dev` workflow). They are authoritative for *why* things were built the way they were; the code is authoritative for *what* exists now.
