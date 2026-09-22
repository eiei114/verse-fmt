use std::io::IsTerminal;

use crate::cli::Color;

pub fn color(mode: Color, stderr: bool) -> bool {
    match mode {
        Color::Always => true,
        Color::Never => false,
        Color::Auto => {
            std::env::var_os("NO_COLOR").is_none()
                && if stderr {
                    std::io::stderr().is_terminal()
                } else {
                    std::io::stdout().is_terminal()
                }
        }
    }
}

pub fn diff(label: &str, before: &str, after: &str, colored: bool) -> String {
    let diff = similar::TextDiff::from_lines(before, after);
    let rendered = diff
        .unified_diff()
        .context_radius(3)
        .header(&format!("a/{label}"), &format!("b/{label}"))
        .to_string();
    let mut result = String::new();
    if before.contains("\r\n") != after.contains("\r\n") {
        result.push_str("# line endings changed (CRLF/LF)\n");
    }
    if colored {
        for line in rendered.split_inclusive('\n') {
            let color = if line.starts_with('+') {
                Some(32)
            } else if line.starts_with('-') {
                Some(31)
            } else {
                None
            };
            if let Some(color) = color {
                result.push_str(&format!("\x1b[{color}m{line}\x1b[0m"));
            } else {
                result.push_str(line);
            }
        }
    } else {
        result.push_str(&rendered);
    }
    result
}
