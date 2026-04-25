# Shipyard

A cross-platform launcher for managing multiple installed versions of Ship of Harkinian (SoH) and related games. Handles downloading releases from GitHub, installing them side-by-side, wiring up ROM paths, and launching the chosen version.

ROMs are imported into a managed library — Shipyard hashes each ROM, identifies it against the per-game ROM database, normalizes endianness, and stores it as `<rom_library_root>/<sha1>.z64`. Set up ROMs under **Settings → Import ROM…**, then assign them to the matching game slot. Configs from earlier versions auto-migrate on first launch.
