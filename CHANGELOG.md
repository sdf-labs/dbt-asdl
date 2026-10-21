# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
