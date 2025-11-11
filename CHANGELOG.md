# Changelog

All notable changes to Kelora will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).

## [Unreleased]

## [0.9.1] - 2025-11-08

### Added

- **New Rhai functions:**
  - `absorb_kv()` - Extract key-value pairs from unstructured text with format preservation
  - `absorb_json()` - Parse JSON embedded in text fields
- **Documentation improvements:**
  - Comprehensive glossary and documentation analysis
  - Rewritten quickstart showcasing custom format parsing
  - Improved tutorial structure and examples
  - Time reference documentation

### Changed

- **Breaking:** `parse_kv()` now skips tokens without separator (more forgiving behavior)
- Improved `--help-functions` terminal output readability
- Enhanced tutorial documentation with better examples and patterns

### Fixed

- Field ordering when only `--exclude-keys` is used
- Function name documentation: `has()` instead of incorrect `has_field()`
- ANSI color rendering in documentation

## [0.9.0] - 2025-11-03

### Added

- **New Rhai functions:**
  - `pluck()` and `pluck_as_nums()` - Extract array elements by index with silent failure handling
  - Micro search helpers for fast substring matching (`find()`, `rfind()`, `contains()`, etc.)
- **CLI flags:**
  - `--no-input` flag to run scripts without reading log input
  - `--normalize-ts` flag to normalize primary timestamps
- **Python-style array slicing** support (e.g., `arr[-1]`, `arr[1:3]`)
- **Raw string literals** support in Rhai scripts for easier regex patterns
- **Fuzzing infrastructure:**
  - JSON parser fuzzing harness with cargo-fuzz
  - Hardened timestamp parsing against edge cases
- **Property-based testing** with proptest for:
  - Format auto-detection
  - Type converters
  - Flattening operations
  - Timestamp handling
- **Documentation improvements:**
  - Raw strings documentation in help text and cheatsheet
  - External tools integration guide (kubectl, docker, pirkle, jtbl)
  - Example data files for `--help-examples`
- **Configuration access:**
  - `conf` object now available in `--end` stage for post-processing

### Changed

- **Breaking:** Renamed `or_unit()` to `or_empty()` for better clarity
- **Breaking:** Renamed `--convert-ts` flag to `--normalize-ts` for consistency
- Simplified help examples to use `??` operator instead of `get_path()`
- Enhanced stats output with better timestamp label alignment
- Condensed README and pointed to full documentation site
- Updated to Rust 2024 edition compatibility

### Fixed

- Timezone inconsistency in result timestamp tracking
- Documentation links and formatting issues
- Chrono duration overflow in relative time calculations
- Timestamp clamping during fuzzing to prevent panics
- Pirkle tool examples and documentation accuracy

## [0.8.1] - 2025-10-28

### Added

- Split integration tests into 18 category-specific modules for better organization
- Enhanced documentation structure to eliminate redundancy

### Changed

- Clarified AI development process across documentation
- Improved multiline documentation with better CLI syntax alignment

## [0.8.0] - 2025-10-25

_First release with git history available. See earlier versions on [crates.io](https://crates.io/crates/kelora/versions)._

## [0.7.2] - 2025-10-22

## [0.7.0] - 2025-10-19

## [0.6.3] - 2025-10-12

## [0.6.1] - 2025-10-08

## [0.6.0] - 2025-10-04

## [0.5.0] - 2025-10-01

## [0.4.0] - 2025-09-23

## [0.3.0] - 2025-09-14

## [0.2.3] - 2025-08-28

## [0.2.2] - 2025-07-27

## [0.2.0] - 2025-07-27

## [0.1.1] - 2025-05-24

## [0.1.0] - 2025-05-24

_Initial release (yanked)._

---

[Unreleased]: https://github.com/dloss/kelora/compare/v0.9.1...HEAD
[0.9.1]: https://github.com/dloss/kelora/compare/v0.9.0...v0.9.1
[0.9.0]: https://github.com/dloss/kelora/compare/v0.8.1...v0.9.0
[0.8.1]: https://github.com/dloss/kelora/compare/v0.8.0...v0.8.1
[0.8.0]: https://github.com/dloss/kelora/releases/tag/v0.8.0
[0.7.2]: https://crates.io/crates/kelora/0.7.2
[0.7.0]: https://crates.io/crates/kelora/0.7.0
[0.6.3]: https://crates.io/crates/kelora/0.6.3
[0.6.1]: https://crates.io/crates/kelora/0.6.1
[0.6.0]: https://crates.io/crates/kelora/0.6.0
[0.5.0]: https://crates.io/crates/kelora/0.5.0
[0.4.0]: https://crates.io/crates/kelora/0.4.0
[0.3.0]: https://crates.io/crates/kelora/0.3.0
[0.2.3]: https://crates.io/crates/kelora/0.2.3
[0.2.2]: https://crates.io/crates/kelora/0.2.2
[0.2.0]: https://crates.io/crates/kelora/0.2.0
[0.1.1]: https://crates.io/crates/kelora/0.1.1
[0.1.0]: https://crates.io/crates/kelora/0.1.0
