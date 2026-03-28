# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/).



## [0.2.2](https://github.com/rvben/vership/compare/v0.2.1...v0.2.2) - 2026-03-28

### Fixed

- **ci**: allow dirty working tree for cargo publish (downloaded artifacts) ([29e9b6c](https://github.com/rvben/vership/commit/29e9b6ced8e6741bec9cd12e830327649a721046))

## [0.2.1](https://github.com/rvben/vership/compare/v0.2.0...v0.2.1) - 2026-03-28

### Fixed

- **ci**: use --zig flag for maturin cross-compilation on GNU targets ([5e65f0b](https://github.com/rvben/vership/commit/5e65f0bc22028467fa77c463c341a85cc853e7b4))

## [0.2.0] - 2026-03-28

### Added

- add --no-push flag to bump command ([9df1baa](https://github.com/rvben/vership/commit/9df1baad4b42307408cdb69a684f4ee28ec3d659))
- **vership**: implement release orchestrator (bump, status, preflight, changelog) ([a782546](https://github.com/rvben/vership/commit/a782546c43f587438ea165005b213e5ba0b8ef3f))
- **vership**: implement config parsing and hook execution ([c325c8c](https://github.com/rvben/vership/commit/c325c8c58853025d76dbfff6d0d10d69d2cfa89e))
- **vership**: implement pre-flight checks (git, lockfile, lint, tests) ([c99e8db](https://github.com/rvben/vership/commit/c99e8db9d728dca548266d8f31dd900d13c5203f))
- **vership**: implement changelog generation from conventional commits ([127fe16](https://github.com/rvben/vership/commit/127fe16c7b675485261fa81a668b3ae70d1328e9))
- **vership**: implement git operations (tags, commits, remote URL, staging) ([12145c3](https://github.com/rvben/vership/commit/12145c3abb64e2bb4bd78f31b1f443cbfa47a175))
- **vership**: implement version parsing, bumping, and project type detection ([32abfbf](https://github.com/rvben/vership/commit/32abfbf4015e8dc3f67767af7afa186a4fe0bb9f))
- **vership**: scaffold project with CLI, error handling, and project type trait ([ad73bfc](https://github.com/rvben/vership/commit/ad73bfcd10418fe00119375cf339976c4078b5ce))

### Fixed

- **vership**: only check tracked files for uncommitted changes ([1dbec7c](https://github.com/rvben/vership/commit/1dbec7cd64a6d9b232d72a39f3dbbab295b5cbec))
- **vership**: address code review findings (TOML parsing, error handling, test coverage) ([a1cbc70](https://github.com/rvben/vership/commit/a1cbc707cf9b0592bb1c331ec93bfe5623bf670f))
