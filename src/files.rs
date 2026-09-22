use std::{
    collections::HashSet,
    fs,
    path::{Component, Path, PathBuf},
};

use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use ignore::gitignore::{Gitignore, GitignoreBuilder};

pub const MAX_TOTAL_BYTES: usize = 64 * 1024 * 1024;

pub fn excludes(patterns: &[String]) -> Result<GlobSet, String> {
    if patterns.len() > 256 {
        return Err("at most 256 exclude globs are supported".into());
    }
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        if pattern.is_empty()
            || pattern.starts_with('!')
            || pattern.starts_with('/')
            || pattern.contains(['\\', ':'])
            || pattern.len() > 4096
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
    if size > 256 * 1024 {
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
}

struct Walker<'a> {
    root: &'a Path,
    excludes: GlobSet,
    seen: HashSet<same_file::Handle>,
    visited: usize,
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
        if self.visited > 100_000 || depth > 128 {
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
                .take(100_001)
                .map(|entry| entry.map(|e| e.path()))
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| format!("{shown}: {e}"))?;
            if entries.len() > 100_000 {
                return Err("directory entry resource limit exceeded".into());
            }
            entries.sort();
            for child in entries {
                if let Err(e) = self.visit(&child, false, ignores, depth + 1) {
                    self.result.errors.push(e);
                }
                if self.visited > 100_000 || self.result.errors.len() >= 100 {
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
                if self.result.paths.len() >= 10_000 {
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
        if walker.visited > 100_000 || walker.result.errors.len() >= 100 {
            break;
        }
    }
    walker
        .result
        .paths
        .sort_by_cached_key(|p| label(p, root).unwrap_or_default());
    if walker.result.paths.is_empty() {
        walker
            .result
            .errors
            .push("no eligible .verse files found".into());
    }
    Ok(walker.result)
}
