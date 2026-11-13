use std::{
    collections::HashSet,
    fs,
    path::{Component, Path, PathBuf},
};

use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use ignore::gitignore::{Gitignore, GitignoreBuilder};

use crate::limits;

pub const MAX_TOTAL_BYTES: usize = 64 * 1024 * 1024;

/// Documented discovery limits. They are named here so a test can assert the
/// boundary that the walk actually enforces instead of repeating a literal.
pub const MAX_VISITED_ENTRIES: usize = 100_000;
pub const MAX_DIRECTORY_DEPTH: usize = 128;
pub const MAX_DIRECTORY_ENTRIES: usize = 100_000;
pub const MAX_SOURCE_FILES: usize = 10_000;
/// Collected discovery diagnostics per invocation; past this, the walk stops
/// reporting more so the report stays bounded.
pub const MAX_DISCOVERY_ERRORS: usize = 100;
pub const MAX_EXCLUDE_GLOBS: usize = 256;
pub const MAX_EXCLUDE_GLOB_BYTES: usize = 4_096;

pub fn excludes(patterns: &[String]) -> Result<GlobSet, String> {
    if patterns.len() > MAX_EXCLUDE_GLOBS {
        return Err("at most 256 exclude globs are supported".into());
    }
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        if pattern.is_empty()
            || pattern.starts_with('!')
            || pattern.starts_with('/')
            || pattern.contains(['\\', ':'])
            || pattern.len() > MAX_EXCLUDE_GLOB_BYTES
        {
            return Err(format!(
                "invalid exclude glob {pattern:?}: use a relative forward-slash pattern without negation"
            ));
        }
        builder.add(
            GlobBuilder::new(pattern)
                .literal_separator(true)
                .backslash_escape(false)
                .case_insensitive(cfg!(windows))
                .build()
                .map_err(|e| e.to_string())?,
        );
    }
    builder.build().map_err(|e| e.to_string())
}

pub fn label(path: &Path, root: &Path) -> Result<String, String> {
    let relative = pathdiff::diff_paths(path, root).unwrap_or_else(|| path.to_path_buf());
    relative
        .to_str()
        .map(|p| p.replace('\\', "/"))
        .ok_or_else(|| "path cannot be represented as Unicode".into())
}

pub fn is_link(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_type().is_symlink() || metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

pub fn guard_path(path: &Path) -> Result<(), String> {
    let absolute = std::path::absolute(path).map_err(|e| e.to_string())?;
    for part in absolute.ancestors() {
        let metadata =
            fs::symlink_metadata(part).map_err(|e| format!("{}: {e}", part.display()))?;
        if is_link(&metadata) {
            return Err(format!(
                "{}: symlinks/junctions/reparse points are unsupported",
                part.display()
            ));
        }
    }
    Ok(())
}

fn protected(path: &Path) -> bool {
    path.components().any(|c| matches!(c, Component::Normal(name) if [".git", "Intermediate", "Saved"].iter().any(|p| name.to_string_lossy().eq_ignore_ascii_case(p))))
        || path.file_name().is_some_and(|n| n.to_string_lossy().to_ascii_lowercase().ends_with(".digest.verse"))
}

fn hidden(path: &Path, metadata: &fs::Metadata) -> bool {
    let dot = path
        .file_name()
        .is_some_and(|name| name.to_string_lossy().starts_with('.'));
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        dot || metadata.file_attributes() & 2 != 0
    }
    #[cfg(not(windows))]
    {
        dot
    }
}

fn add_ignore(directory: &Path, ignores: &mut Vec<Gitignore>) -> Result<bool, String> {
    let file = directory.join(".gitignore");
    if !file.try_exists().map_err(|e| e.to_string())? {
        return Ok(false);
    }
    let size = fs::metadata(&file).map_err(|e| e.to_string())?.len();
    if size > limits::CONFIG_BYTES as u64 {
        return Err(format!("{}: .gitignore exceeds 256 KiB", file.display()));
    }
    let mut builder = GitignoreBuilder::new(directory);
    if let Some(e) = builder.add(&file) {
        return Err(format!("{}: {e}", file.display()));
    }
    ignores.push(builder.build().map_err(|e| e.to_string())?);
    Ok(true)
}

#[derive(Default)]
pub struct Discovery {
    pub paths: Vec<PathBuf>,
    pub errors: Vec<String>,
    pub excluded: Vec<String>,
    /// Set when the walk stopped because a documented resource limit was
    /// reached rather than because one path was unusable. The distinction
    /// matters to the caller: a limit leaves `paths` holding every source found
    /// before the limit, and the documented partial-completion contract says
    /// those sources still report their own result. A plain discovery error
    /// (a missing or excluded input) keeps the invocation's previous behaviour
    /// of analyzing nothing.
    pub limit_exceeded: bool,
}

struct Walker<'a> {
    root: &'a Path,
    excludes: GlobSet,
    seen: HashSet<same_file::Handle>,
    visited: usize,
    max_directory_entries: usize,
    result: Discovery,
}

impl Walker<'_> {
    fn visit(
        &mut self,
        path: &Path,
        explicit: bool,
        ignores: &mut Vec<Gitignore>,
        depth: usize,
    ) -> Result<(), String> {
        self.visited += 1;
        if self.visited > MAX_VISITED_ENTRIES || depth > MAX_DIRECTORY_DEPTH {
            self.result.limit_exceeded = true;
            return Err("directory traversal resource limit exceeded".into());
        }
        let metadata =
            fs::symlink_metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let shown = label(path, self.root)?;
        let mut reason = None;
        if is_link(&metadata) {
            reason = Some("symlink/junction/reparse point");
        } else if protected(path) {
            reason = Some("generated file/directory or Git metadata protection");
        } else if self.excludes.is_match(&shown) {
            reason = Some("verse.toml exclude");
        } else if !explicit {
            if hidden(path, &metadata) {
                reason = Some("hidden entry");
            } else {
                let mut ignored = false;
                for matcher in ignores.iter() {
                    let matched = matcher.matched_path_or_any_parents(path, metadata.is_dir());
                    if matched.is_ignore() {
                        ignored = true;
                    } else if matched.is_whitelist() {
                        ignored = false;
                    }
                }
                if ignored {
                    reason = Some(".gitignore");
                }
            }
        }
        if let Some(reason) = reason {
            let message = format!("{shown}: excluded ({reason})");
            if explicit {
                return Err(message);
            }
            self.result.excluded.push(message);
            return Ok(());
        }
        if metadata.is_dir() {
            let added = add_ignore(path, ignores)?;
            let mut entries = fs::read_dir(path)
                .map_err(|e| format!("{shown}: {e}"))?
                .take(self.max_directory_entries + 1)
                .map(|entry| entry.map(|e| e.path()))
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| format!("{shown}: {e}"))?;
            if entries.len() > self.max_directory_entries {
                self.result.limit_exceeded = true;
                return Err("directory entry resource limit exceeded".into());
            }
            entries.sort();
            for child in entries {
                if let Err(e) = self.visit(&child, false, ignores, depth + 1) {
                    self.result.errors.push(e);
                }
                if self.result.limit_exceeded
                    || self.visited > MAX_VISITED_ENTRIES
                    || self.result.errors.len() >= MAX_DISCOVERY_ERRORS
                {
                    break;
                }
            }
            if added {
                ignores.pop();
            }
        } else if metadata.is_file()
            && path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("verse"))
        {
            let identity =
                same_file::Handle::from_path(path).map_err(|e| format!("{shown}: {e}"))?;
            if !self.seen.contains(&identity) {
                if self.result.paths.len() >= MAX_SOURCE_FILES {
                    self.result.limit_exceeded = true;
                    return Err("at most 10000 source files are supported".into());
                }
                self.seen.insert(identity);
                self.result.paths.push(path.to_path_buf());
            }
        } else if explicit {
            return Err(format!("{shown}: expected a .verse file or directory"));
        }
        Ok(())
    }
}

pub fn discover(inputs: &[PathBuf], root: &Path, patterns: &[String]) -> Result<Discovery, String> {
    let mut walker = Walker {
        root,
        excludes: excludes(patterns)?,
        seen: HashSet::new(),
        visited: 0,
        max_directory_entries: MAX_DIRECTORY_ENTRIES,
        result: Discovery::default(),
    };
    for input in inputs {
        let result = (|| {
            guard_path(input)?;
            let path = fs::canonicalize(input).map_err(|e| format!("{}: {e}", input.display()))?;
            let mut ancestors = Vec::new();
            // A traversal root that itself contains .git must not inherit ignores above that root.
            if !path.is_dir() || !path.join(".git").exists() {
                for ancestor in path.parent().into_iter().flat_map(Path::ancestors) {
                    ancestors.push(ancestor);
                    if ancestor.join(".git").exists() {
                        break;
                    }
                }
            }
            let mut ignores = Vec::new();
            for ancestor in ancestors.into_iter().rev() {
                add_ignore(ancestor, &mut ignores)?;
            }
            walker.visit(&path, true, &mut ignores, 0)
        })();
        if let Err(e) = result {
            walker.result.errors.push(e);
        }
        if walker.result.limit_exceeded
            || walker.visited > MAX_VISITED_ENTRIES
            || walker.result.errors.len() >= MAX_DISCOVERY_ERRORS
        {
            break;
        }
    }
    walker
        .result
        .paths
        .sort_by_cached_key(|p| label(p, root).unwrap_or_default());
    if walker.result.paths.is_empty() && !walker.result.limit_exceeded {
        walker
            .result
            .errors
            .push("no eligible .verse files found".into());
    }
    Ok(walker.result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn walker(visited: usize) -> Walker<'static> {
        walker_with_directory_limit(Path::new("."), visited, MAX_DIRECTORY_ENTRIES)
    }

    fn walker_with_directory_limit<'a>(
        root: &'a Path,
        visited: usize,
        max_directory_entries: usize,
    ) -> Walker<'a> {
        Walker {
            root,
            excludes: GlobSetBuilder::new().build().unwrap(),
            seen: HashSet::new(),
            visited,
            max_directory_entries,
            result: Discovery::default(),
        }
    }

    /// The boundary is "one past the limit fails, at the limit still works",
    /// not merely "the message exists". The counter and depth are injected
    /// directly so the test proves the predicate without creating 100,000 real
    /// filesystem entries; `tests/resource-limits.rs` covers the observable CLI
    /// behaviour for the limits that a real directory can reach cheaply.
    #[test]
    fn visited_entry_limit_fails_before_accessing_another_path() {
        let mut state = walker(MAX_VISITED_ENTRIES);
        let error = state
            .visit(
                Path::new("path-is-not-read-after-limit"),
                false,
                &mut Vec::new(),
                0,
            )
            .unwrap_err();

        assert!(error.contains("directory traversal resource limit"));
        assert_eq!(state.visited, MAX_VISITED_ENTRIES + 1);
    }

    #[test]
    fn visited_entry_limit_stops_before_later_siblings() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let before = root.join("a-before.verse");
        let trigger = root.join("b-trigger.verse");
        let after = root.join("c-after.verse");
        for path in [&before, &trigger, &after] {
            std::fs::write(path, b"A := 1\n").unwrap();
        }

        let mut state =
            walker_with_directory_limit(root, MAX_VISITED_ENTRIES - 2, MAX_DIRECTORY_ENTRIES);
        state
            .visit(root, true, &mut Vec::new(), 0)
            .expect("the parent directory itself is within the visited-entry limit");

        assert!(state.result.limit_exceeded);
        assert_eq!(state.result.paths, vec![before]);
        assert_eq!(state.result.errors.len(), 1);
    }

    #[test]
    fn directory_entry_limit_stops_before_later_siblings() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let crowded = root.join("a-crowded");
        std::fs::create_dir(&crowded).unwrap();
        for name in ["one.verse", "two.verse", "three.verse"] {
            std::fs::write(crowded.join(name), b"A := 1\n").unwrap();
        }
        let after = root.join("z-after.verse");
        std::fs::write(&after, b"A := 1\n").unwrap();

        let mut state = walker_with_directory_limit(root, 0, 2);
        state
            .visit(root, true, &mut Vec::new(), 0)
            .expect("the child directory records its own limit error");

        assert!(state.result.limit_exceeded);
        assert!(state.result.paths.is_empty());
        assert_eq!(state.result.errors.len(), 1);
        assert!(state.result.errors[0].contains("directory entry resource limit"));
        assert!(!state.result.paths.contains(&after));
    }

    #[test]
    fn depth_limit_stops_siblings_and_later_inputs() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let scan_root = root.join("scan");
        std::fs::create_dir(&scan_root).unwrap();
        let before = scan_root.join("a-before.verse");
        std::fs::write(&before, b"A := 1\n").unwrap();

        let mut too_deep = scan_root.clone();
        for _ in 0..=MAX_DIRECTORY_DEPTH {
            too_deep.push("x");
            std::fs::create_dir(&too_deep).unwrap();
        }
        std::fs::write(too_deep.join("deep.verse"), b"A := 1\n").unwrap();

        let after_sibling = scan_root.join("z-after.verse");
        std::fs::write(&after_sibling, b"A := 1\n").unwrap();
        let later_input = root.join("zz-later.verse");
        std::fs::write(&later_input, b"A := 1\n").unwrap();

        let scan_root = fs::canonicalize(scan_root).unwrap();
        let before = fs::canonicalize(before).unwrap();
        let after_sibling = fs::canonicalize(after_sibling).unwrap();
        let later_input = fs::canonicalize(later_input).unwrap();
        let result = discover(&[scan_root, later_input.clone()], root, &[]).unwrap();

        assert!(result.limit_exceeded);
        assert_eq!(result.paths, vec![before]);
        assert!(!result.paths.contains(&after_sibling));
        assert!(!result.paths.contains(&later_input));
    }

    #[test]
    fn directory_depth_limit_fails_before_accessing_another_path() {
        let mut state = walker(0);
        let error = state
            .visit(
                Path::new("path-is-not-read-after-limit"),
                false,
                &mut Vec::new(),
                MAX_DIRECTORY_DEPTH + 1,
            )
            .unwrap_err();

        assert!(error.contains("directory traversal resource limit"));
        assert_eq!(state.visited, 1);
    }

    /// The last permitted entry and depth both still resolve a normal file, so
    /// the guard is off-by-one-safe rather than "fails somewhere near the cap".
    #[test]
    fn the_last_permitted_visited_entry_and_depth_are_accepted() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("a.verse");
        std::fs::write(&path, b"A := 1\n").unwrap();

        let mut at_limit = walker(MAX_VISITED_ENTRIES - 1);
        at_limit
            .visit(&path, false, &mut Vec::new(), MAX_DIRECTORY_DEPTH)
            .unwrap();
        assert_eq!(at_limit.visited, MAX_VISITED_ENTRIES);
        assert_eq!(at_limit.result.paths, vec![path.clone()]);

        // One step further on either counter is the documented refusal.
        let mut over_visited = walker(MAX_VISITED_ENTRIES);
        assert!(
            over_visited
                .visit(&path, false, &mut Vec::new(), 0)
                .unwrap_err()
                .contains("directory traversal resource limit")
        );
        let mut over_deep = walker(0);
        assert!(
            over_deep
                .visit(&path, false, &mut Vec::new(), MAX_DIRECTORY_DEPTH + 1)
                .unwrap_err()
                .contains("directory traversal resource limit")
        );
    }

    /// The source-count guard is on `result.paths`, not on the visited counter,
    /// so the boundary is injected as path entries — no 10,000 files on disk.
    #[test]
    fn the_source_count_limit_stops_only_the_next_distinct_file() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("last.verse");
        std::fs::write(&path, b"A := 1\n").unwrap();

        let mut at_limit = walker(0);
        at_limit.result.paths = (0..MAX_SOURCE_FILES - 1)
            .map(|index| PathBuf::from(format!("source-{index:05}.verse")))
            .collect();
        at_limit.visit(&path, false, &mut Vec::new(), 0).unwrap();
        assert_eq!(at_limit.result.paths.len(), MAX_SOURCE_FILES);

        let mut past_limit = walker(0);
        past_limit.result.paths = (0..MAX_SOURCE_FILES)
            .map(|index| PathBuf::from(format!("source-{index:05}.verse")))
            .collect();
        assert!(
            past_limit
                .visit(&path, false, &mut Vec::new(), 0)
                .unwrap_err()
                .contains("at most 10000 source files")
        );
        // A rejected file is not counted, so the already discovered set stays
        // exactly at the limit.
        assert_eq!(past_limit.result.paths.len(), MAX_SOURCE_FILES);
    }

    #[test]
    fn source_count_limit_stops_before_later_siblings() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let trigger = root.join("a-trigger.verse");
        let after = root.join("z-after.verse");
        std::fs::write(&trigger, b"A := 1\n").unwrap();
        std::fs::write(&after, b"A := 1\n").unwrap();

        let mut state = walker_with_directory_limit(root, 0, MAX_DIRECTORY_ENTRIES);
        state.result.paths = (0..MAX_SOURCE_FILES)
            .map(|index| PathBuf::from(format!("source-{index:05}.verse")))
            .collect();
        state
            .visit(root, true, &mut Vec::new(), 0)
            .expect("the root records its child limit error");

        assert!(state.result.limit_exceeded);
        assert_eq!(state.result.paths.len(), MAX_SOURCE_FILES);
        assert_eq!(state.result.errors.len(), 1);
        assert!(!state.result.paths.contains(&after));
    }
}
