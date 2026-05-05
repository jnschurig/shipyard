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

**Per-game pluggable model.** Everything game-specific is behind the `games::Game` trait (`src/games/mod.rs`). SoH is the only current implementation (`src/games/soh.rs`) but the registry is multi-game-ready. When adding a game-specific behavior, the answer is almost always "add it to the `Game` trait", not "branch on game slug".

**Slot identifiers are per-game `&'static str`.** `SlotSpec::id` and `CachedAssetSpec::slot_id` share the same identifier space within a game. `Config.slot_assignments` is `HashMap<game_slug, HashMap<slot_id, rom_filename>>`. Do not introduce a global enum of slots.

**ROM library + launch wiring.** Imported ROMs are plain copies in `<config_dir>/roms/<original_filename>` (no hashing, no validation, no format detection — trust the user's pick). Slot assignments map a slot to a ROM filename. At launch, `roms::wiring::reconcile` creates `<install_dir>/<slot.symlink_filename>` as a symlink — but only when the corresponding cached `.o2r`/`.otr` is missing for that slot. Once the game has generated its baked archive, reconcile is a no-op for that slot. Launch passes no ROM-related CLI args; the symlink is the only wiring mechanism.

**Config migration is split into two phases.** `Config::load_from` is pure: it returns a `LoadedConfig` whose `pending_migration: Option<PendingMigration>` describes filesystem work that needs to happen (ROM imports). `main.rs` (and tests) materialize the migration via `roms::library::apply_pending_migration`, then `Config::save_to` to persist. Don't add filesystem side effects to `Config::load_from`.

**Platform abstraction is OS-only.** `platform::Platform` covers `default_library_root`, `config_dir`, `cache_dir`, `extract`, `asset_keyword`. Asset selection and launch commands belong on `Game`, not `Platform`. macOS extracts via `hdiutil` mount with a RAII detach guard; Linux drops the AppImage in place + chmod. Windows is not a target yet.

**GitHub client persists ETags** to `<cache_dir>/etags.json` so cold restarts don't burn the 60/hr anonymous rate limit. `GITHUB_TOKEN` env var is the user-facing escape hatch.

**Atomic writes everywhere.** Configs, manifests, installs (`<dest>.partial` → rename), ROM imports, symlinks (`symlink → rename`). When adding a write, follow the same pattern.

## Conventions worth knowing

- Banner-driven error UX: surface failures by pushing onto `App.banners`, not modal dialogs (modals are reserved for destructive confirms like clear-cache).
- Tests prefer real filesystems via `tempfile::tempdir()` and real subprocesses (`/usr/bin/true`, shell shims) over mocks. `wiremock` is used for HTTP only.
- The launcher detaches via `setsid()` (Unix `pre_exec`) so closing Shipyard doesn't kill running games.
- Long-form design docs live in `.local/docs/<feature>/{01-requirements,02-plan}.md` (the `/spec-and-dev` workflow). They are authoritative for *why* things were built the way they were; the code is authoritative for *what* exists now.
