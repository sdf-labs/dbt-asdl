# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.0.0](https://github.com/sdf-labs/dbt-asdl/compare/v0.3.0...v1.0.0) - 2026-10-22

### Other

- Document Rust and C++ tree build and API usage in README ([#24](https://github.com/sdf-labs/dbt-asdl/pull/24))
- Add C++ flat AST codegen for typed record structs and views ([#23](https://github.com/sdf-labs/dbt-asdl/pull/23))
- Package C++ runtime header under include/dbt_asdl with Rust/CLI helpers and CMake target ([#22](https://github.com/sdf-labs/dbt-asdl/pull/22))
- Add C++ shim layer for FlatTree and static FFI integration tests ([#20](https://github.com/sdf-labs/dbt-asdl/pull/20))

## [0.3.0](https://github.com/sdf-labs/dbt-asdl/compare/v0.2.0...v0.3.0) - 2026-10-21

### Other

- Bump RUNTIME_ABI_VERSION. ([#19](https://github.com/sdf-labs/dbt-asdl/pull/19))
- Optimize host-side Tree, Node, SeqView, and Value layouts ([#18](https://github.com/sdf-labs/dbt-asdl/pull/18))
- Optimize flat ABI schema descriptor and header layouts ([#17](https://github.com/sdf-labs/dbt-asdl/pull/17))
- Fold FlatNode into packed u32 sum references ([#16](https://github.com/sdf-labs/dbt-asdl/pull/16))
- Optimize flat ABI optional scalar layouts ([#14](https://github.com/sdf-labs/dbt-asdl/pull/14))

## [0.2.0](https://github.com/sdf-labs/dbt-asdl/compare/v0.1.0...v0.2.0) - 2026-10-21

### Other

- Specify ABI_VERSION in ASDL module header and add version drift tests ([#13](https://github.com/sdf-labs/dbt-asdl/pull/13))
- Add cross-library flat ABI integration tests ([#12](https://github.com/sdf-labs/dbt-asdl/pull/12))
- Add end-to-end integration tests for mini_sql.asdl ([#11](https://github.com/sdf-labs/dbt-asdl/pull/11))
- Lift flat_runtime into dbt-asdl library ([#10](https://github.com/sdf-labs/dbt-asdl/pull/10))
- Add an `emit!` macro and use it for all codegen output ([#9](https://github.com/sdf-labs/dbt-asdl/pull/9))
- add release-plz release workflows
- Rename the flat ABI types from `Sql*` to `Flat*` ([#6](https://github.com/sdf-labs/dbt-asdl/pull/6))
- Make generated Rust code self-contained and reorganize public API ([#5](https://github.com/sdf-labs/dbt-asdl/pull/5))
- Add flat #[repr(C)] Rust AST code generator (rust::generate_flat)
- Bump version to 0.2.0 and untrack Cargo.lock
