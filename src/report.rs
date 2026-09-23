use std::io::IsTerminal;

use crate::cli::Color;

pub fn color(mode: Color, stderr: bool) -> bool {
    match mode {
        Color::Always => true,
        Color::Never => false,
        Color::Auto => {
            if std::env::var_os("NO_COLOR").is_some() {
                return false;
            }
            let is_terminal = if stderr {
                std::io::stderr().is_terminal()
            } else {
                std::io::stdout().is_terminal()
            };
            is_terminal && enable_virtual_terminal_processing(stderr)
        }
    }
}

#[cfg(windows)]
fn enable_virtual_terminal_processing(stderr: bool) -> bool {
    use windows_sys::Win32::{
        Foundation::INVALID_HANDLE_VALUE,
        System::Console::{
            ENABLE_VIRTUAL_TERMINAL_PROCESSING, GetConsoleMode, GetStdHandle, STD_ERROR_HANDLE,
            STD_OUTPUT_HANDLE, SetConsoleMode,
        },
    };

    let standard_handle = if stderr {
        STD_ERROR_HANDLE
    } else {
        STD_OUTPUT_HANDLE
    };
    // GetStdHandle may return NULL or INVALID_HANDLE_VALUE when the stream is unavailable.
    let handle = unsafe { GetStdHandle(standard_handle) };
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        return false;
    }

    let mut mode = 0;
    if unsafe { GetConsoleMode(handle, &mut mode) } == 0 {
        return false;
    }
    if mode & ENABLE_VIRTUAL_TERMINAL_PROCESSING != 0 {
        return true;
    }
    unsafe { SetConsoleMode(handle, mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING) != 0 }
}

#[cfg(not(windows))]
fn enable_virtual_terminal_processing(_stderr: bool) -> bool {
    true
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
