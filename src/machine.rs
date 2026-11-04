//! Machine-report hygiene.
//!
//! Diagnostics built by `files.rs` and `write.rs` embed absolute paths, and the
//! human text path keeps printing them exactly as it always has. The report
//! contract forbids absolute paths in `reason` and `executionErrors`, so every
//! machine copy of a diagnostic passes through [`sanitize`], which rewrites the
//! absolute spellings this invocation can have printed into their root-relative
//! labels.
use std::path::Path;

/// Characters that end a path inside a diagnostic: what Windows forbids in a
/// name, plus a newline. Used to find where a rewritten path stops, so only its
/// own separators are normalized and the prose that follows is untouched.
const PATH_END: [char; 9] = ['<', '>', ':', '"', '|', '?', '*', '\n', '\r'];

/// Machine-safe form of one diagnostic. Absolute paths become `/`-separated
/// labels relative to `root`; a path the root cannot relativize (another
/// Windows volume) is masked instead of leaking.
pub fn sanitize(message: &str, root: &Path) -> String {
    let mut text = unverbatim(message);
    for (spelling, replacement) in prefixes(root) {
        text = rewrite(&text, &spelling, &replacement);
    }
    mask_absolute(&text)
}

/// Drops the Windows verbatim prefixes `fs::canonicalize` returns, so the path
/// text in a diagnostic matches the spellings [`prefixes`] can generate from an
/// ordinary configuration root.
fn unverbatim(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut index = 0;
    while index < text.len() {
        let rest = &text[index..];
        if starts_a_token(text, index) {
            if rest.starts_with(r"\\?\UNC\") {
                result.push_str("\\\\");
                index += r"\\?\UNC\".len();
                continue;
            }
            if rest.starts_with(r"\\?\") {
                index += r"\\?\".len();
                continue;
            }
        }
        let character = rest.chars().next().expect("rest is non-empty");
        result.push(character);
        index += character.len_utf8();
    }
    result
}

/// Absolute spellings this invocation can have printed, longest first.
///
/// The root and its ancestors are enough: every absolute path on the same
/// volume starts with one of them, so a nested file, a guard ancestor or a
/// random staging directory name is covered without guessing which paths were
/// involved. Each ancestor is offered in native, `/`-separated and
/// `Debug`-escaped form, bare (the directory itself) and followed by a
/// separator (anything below it).
fn prefixes(root: &Path) -> Vec<(String, String)> {
    let mut found: Vec<(String, String)> = Vec::new();
    for ancestor in root.ancestors().filter(|path| path.is_absolute()) {
        let label = relative_label(ancestor, root);
        // The message has already had its verbatim prefix dropped, so the root
        // spelling has to match that form: `fs::canonicalize` hands back
        // `\\?\C:\...` on Windows.
        let native = unverbatim(&ancestor.to_string_lossy());
        for spelling in [
            native.clone(),
            native.replace('\\', "/"),
            native.replace('\\', "\\\\"),
        ] {
            let bare = if ends_with_separator(&spelling) {
                joined(&label)
            } else {
                label.clone()
            };
            found.push((spelling.clone(), bare));
            for separator in ["\\", "/", "\\\\"] {
                found.push((format!("{spelling}{separator}"), joined(&label)));
            }
        }
    }
    // Longest spelling first, so a root is rewritten before an ancestor of it.
    found.sort_by_key(|(spelling, _)| std::cmp::Reverse(spelling.len()));
    found.dedup_by(|left, right| left.0 == right.0);
    found
}

fn ends_with_separator(spelling: &str) -> bool {
    spelling.ends_with('\\') || spelling.ends_with('/')
}

/// Replacement for a matched prefix followed by a separator.
fn joined(label: &str) -> String {
    if label.is_empty() {
        String::new()
    } else {
        format!("{label}/")
    }
}

fn relative_label(path: &Path, root: &Path) -> String {
    let label = pathdiff::diff_paths(path, root)
        .map(|relative| relative.to_string_lossy().replace('\\', "/"))
        .unwrap_or_default();
    if label == "." { String::new() } else { label }
}

/// Replaces every whole-path occurrence of `spelling`, then normalizes the
/// separators of the rest of that path.
fn rewrite(text: &str, spelling: &str, replacement: &str) -> String {
    if spelling.is_empty() || !text.contains(spelling) {
        return text.to_owned();
    }
    let mut result = String::with_capacity(text.len());
    let mut index = 0;
    while let Some(found) = text[index..].find(spelling) {
        let start = index + found;
        let after = start + spelling.len();
        if !starts_a_token(text, start) || !ends_a_path(spelling, text, after) {
            result.push_str(&text[index..after]);
            index = after;
            continue;
        }
        result.push_str(&text[index..start]);
        result.push_str(replacement);
        index = after;
        index += copy_path_tail(text, index, &mut result);
    }
    result.push_str(&text[index..]);
    result
}

/// A match that does not end in a separator must be followed by one, or by the
/// end of the path: otherwise `C:\work\ver` would rewrite the sibling
/// `C:\work\ver-sub\x.verse` into a label that names a different file. A
/// rejected match stays in place, so the parent spelling (`C:\work\` → `../`)
/// still relativizes it correctly on a later pass.
fn ends_a_path(spelling: &str, text: &str, after: usize) -> bool {
    if ends_with_separator(spelling) {
        return true;
    }
    match text[after..].chars().next() {
        None => true,
        Some(next) => next == '\\' || next == '/' || PATH_END.contains(&next),
    }
}

/// A path starts a token: it begins the diagnostic or follows a delimiter.
/// Without this a prefix that happens to occur inside a longer name would be
/// rewritten.
fn starts_a_token(text: &str, start: usize) -> bool {
    match text[..start].chars().next_back() {
        None => true,
        Some(previous) => matches!(
            previous,
            ' ' | '\t' | '"' | '\'' | '(' | '[' | '{' | '<' | '=' | ',' | ';'
        ),
    }
}

/// Copies the separators of the path that follows a match as `/` and returns
/// how many bytes were consumed, so the rest of the diagnostic is left alone.
/// A run of separators becomes one `/`, which keeps a `Debug`-escaped path
/// (`C:\\a\\b`) from turning into a doubled form.
fn copy_path_tail(text: &str, index: usize, out: &mut String) -> usize {
    let rest = &text[index..];
    let end = rest
        .char_indices()
        .find(|(_, character)| PATH_END.contains(character))
        .map_or(rest.len(), |(offset, _)| offset);
    let mut separator = false;
    for character in rest[..end].chars() {
        if character == '\\' || character == '/' {
            if !separator {
                out.push('/');
                separator = true;
            }
        } else {
            out.push(character);
            separator = false;
        }
    }
    end
}

/// Last resort for a path the prefix set cannot relativize, such as a target on
/// another volume: mask the absolute prefix so the report never carries one.
fn mask_absolute(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = absolute_start(rest) {
        result.push_str(&rest[..start]);
        result.push_str("<abs>/");
        // Skip the drive letter and its separator, or the two leading
        // backslashes of a UNC share.
        let skip = if rest[start + 1..].starts_with(':') {
            start
                + 2
                + usize::from(matches!(
                    rest.as_bytes().get(start + 2),
                    Some(b'\\') | Some(b'/')
                ))
        } else {
            start + 2
        };
        let tail = skip + copy_path_tail(rest, skip, &mut result);
        rest = &rest[tail..];
    }
    result.push_str(rest);
    result
}

fn absolute_start(text: &str) -> Option<usize> {
    for (index, byte) in text.bytes().enumerate() {
        if !byte.is_ascii() || !starts_a_token(text, index) {
            continue;
        }
        let rest = &text[index..];
        let mut bytes = rest.bytes();
        let drive = bytes.next().is_some_and(|b| b.is_ascii_alphabetic())
            && bytes.next() == Some(b':')
            && matches!(bytes.next(), Some(b'\\') | Some(b'/'));
        if drive || rest.starts_with("\\\\") {
            return Some(index);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> std::path::PathBuf {
        std::env::temp_dir().join("verse-fmt-machine-root")
    }

    fn spell(root: &Path, relative: &str) -> String {
        root.join(relative).display().to_string()
    }

    #[test]
    fn relativizes_the_root_and_everything_below_it() {
        let root = root();
        assert_eq!(
            sanitize(&format!("{}: refused", spell(&root, "sub/a.verse")), &root),
            "sub/a.verse: refused"
        );
        assert_eq!(
            sanitize(&format!("{}: read-only file", root.display()), &root),
            ": read-only file"
        );
        // A staging directory and its random name are below the root.
        assert_eq!(
            sanitize(
                &format!("staged {}", spell(&root, ".verse-write-abc")),
                &root
            ),
            "staged .verse-write-abc"
        );
    }

    #[test]
    fn a_label_keeps_its_spaces_and_mixed_separators_become_slashes() {
        let root = root();
        let document = root.join("日本語 project").join("nested folder/a.verse");
        assert_eq!(
            sanitize(&format!("{}: denied", document.display()), &root),
            "日本語 project/nested folder/a.verse: denied"
        );
    }

    #[test]
    fn paths_above_the_root_become_dot_dot_labels() {
        let root = root();
        let sibling = root.parent().unwrap().join("other");
        let label = sanitize(&format!("{}: symlink", sibling.display()), &root);
        assert!(label.starts_with("../other"), "{label}");
        assert!(!label.contains(&root.display().to_string()), "{label}");
    }

    #[test]
    fn debug_escaped_paths_are_rewritten_too() {
        let root = root();
        let document = root.join("sub/a.verse");
        let message = format!("retained {document:?}");
        let sanitized = sanitize(&message, &root);
        assert!(
            !sanitized.contains(&root.display().to_string()),
            "{sanitized}"
        );
        assert!(!sanitized.contains('\\'), "{sanitized}");
    }

    #[test]
    fn a_sibling_directory_sharing_the_root_prefix_is_not_mistaken_for_the_root() {
        let root = root();
        let sibling = root.with_file_name(format!(
            "{}-sibling",
            root.file_name().unwrap().to_string_lossy()
        ));
        let document = sibling.join("a.verse");
        let sanitized = sanitize(&format!("{}: denied", document.display()), &root);
        assert!(
            !sanitized.contains(&root.display().to_string()),
            "{sanitized}"
        );
        assert!(!sanitized.contains('\\'), "{sanitized}");
        assert!(
            sanitized.contains("sibling/a.verse"),
            "the sibling must keep its own name: {sanitized}"
        );
    }

    /// Masking is a last resort for text that still looks like an absolute
    /// path. It can also mask a rejected configuration value that happens to be
    /// spelled like one (an exclude glob with a drive prefix is already invalid
    /// and printed via `Debug`); stderr keeps the exact wording either way, and
    /// the machine copy still never carries an absolute path.
    #[cfg(windows)]
    #[test]
    fn masking_never_leaves_an_absolute_path_behind() {
        let root = Path::new("C:\\work\\verse-fmt");
        for message in [
            "invalid exclude glob \"D:\\\\foo\"",
            "invalid exclude glob \"\\\\\\\\server\"",
            "retained Some(\"D:\\\\other\\\\x.verse\")",
        ] {
            let sanitized = sanitize(message, root);
            assert!(
                !sanitized.contains("D:\\\\") || sanitized.starts_with("invalid"),
                "{sanitized}"
            );
            assert!(!sanitized.contains("C:\\\\"), "{sanitized}");
        }
    }

    #[test]
    fn a_diagnostic_without_paths_is_unchanged() {
        let root = root();
        for message in [
            "1:1: unterminated block comment",
            "formatter output differs from the source",
            "invalid exclude glob \"a\\\\b\": use a relative forward-slash pattern",
        ] {
            assert_eq!(sanitize(message, &root), message);
        }
    }

    #[cfg(windows)]
    #[test]
    fn an_absolute_path_the_root_cannot_relativize_is_masked() {
        let root = Path::new("C:\\work\\verse-fmt");
        assert_eq!(
            sanitize("D:\\other\\x.verse: denied", root),
            "<abs>/other/x.verse: denied"
        );
        assert_eq!(
            sanitize("\\\\server\\share\\x.verse: denied", root),
            "<abs>/server/share/x.verse: denied"
        );
    }

    /// `fs::canonicalize` returns verbatim Windows paths, and the text path
    /// prints them. The machine copy must relativize those too, whether or not
    /// the configuration root was canonicalized.
    #[cfg(windows)]
    #[test]
    fn a_verbatim_path_is_relativized_against_a_plain_root() {
        let root = Path::new("C:\\work\\ver");
        assert_eq!(
            sanitize("\\\\?\\C:\\work\\ver\\sub\\a.verse: read-only file", root),
            "sub/a.verse: read-only file"
        );
    }

    #[cfg(windows)]
    #[test]
    fn a_verbatim_path_is_relativized_against_a_verbatim_root() {
        let root = Path::new("\\\\?\\C:\\work\\ver");
        assert_eq!(
            sanitize("\\\\?\\C:\\work\\ver\\sub\\a.verse: read-only file", root),
            "sub/a.verse: read-only file"
        );
    }
}
