//! Every documented resource limit, in one place.
//!
//! Documentation quotes these values (`README.md`, `docs/cli.md`,
//! `docs/parser-design.md`, `docs/grammar-coverage.md` and
//! `docs/resource-limits.md`), and `tests/docs-drift.rs` reads this file
//! directly. A document and its constant therefore cannot drift apart silently:
//! the guard fails with the document, the claim and both values.
//!
//! The canonical sentence is the shared toolchain contract in the maintainer
//! vault, which lives outside both repositories and is human-owned, so no
//! in-repository test can read it. This repository's restatements are what the
//! guard enforces; `VERSE_FMT_CONTRACT_DOC` additionally lets a developer point
//! the guard at the vault copy.
//!
//! This file must stay dependency-free so both the binary and the integration
//! tests can compile it without a library wrapper.

use std::time::Duration;

/// Largest accepted Verse source, one file.
pub const MAX_SOURCE_BYTES: usize = 8 * 1024 * 1024;
/// Largest accepted total input per invocation.
pub const MAX_TOTAL_BYTES: usize = 64 * 1024 * 1024;
/// Deepest accepted lexical/bracket nesting level.
pub const MAX_NESTING: usize = 256;
/// Largest accepted `.verse.toml` or ignore file.
pub const CONFIG_BYTES: usize = 256 * 1024;
/// Most sources one invocation analyzes.
pub const MAX_SOURCE_FILES: usize = 10_000;
/// Most filesystem entries the walk visits.
pub const MAX_VISITED_ENTRIES: usize = 100_000;
/// Deepest directory nesting counted from the config root.
pub const MAX_DIRECTORY_DEPTH: usize = 128;
/// Most entries the walk reads from a single directory.
pub const MAX_DIRECTORY_ENTRIES: usize = 100_000;
/// Most collected discovery diagnostics.
pub const MAX_DISCOVERY_ERRORS: usize = 100;
/// Most configured exclude globs.
pub const MAX_EXCLUDE_GLOBS: usize = 256;
/// Longest accepted exclude glob.
pub const MAX_EXCLUDE_GLOB_BYTES: usize = 4_096;
/// Most lexical tokens one source may produce.
pub const MAX_TOKENS: usize = 250_000;
/// Longest single parse before it is refused.
pub const MAX_PARSE_BUDGET: Duration = Duration::from_secs(2);
/// Deepest CST walk level.
pub const MAX_CST_DEPTH: usize = 512;
/// Most fingerprint events the shape walk may emit.
pub const MAX_FINGERPRINT_EVENTS: usize = 1_000_000;
