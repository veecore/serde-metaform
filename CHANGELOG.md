# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),  
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.0.1] - 2025-09-19

### Fixed
- Resolved a compile error caused by an incomplete trait implementation for the internal writer.
- Fixed a potential compiler stack overflow when serializing deeply nested or recursive data structures (e.g., `serde_json::Value`). The internal writer logic was refactored to prevent unbounded recursion during Rust's trait evaluation process.

---

## \[1.0.0] - 2025-10-07

### Added

* Initial release