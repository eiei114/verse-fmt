use std::path::PathBuf;

use clap::{Parser, ValueEnum};

#[derive(Clone, Copy, Debug, Default, ValueEnum)]
pub enum Color {
    #[default]
    Auto,
    Always,
    Never,
}

#[derive(Debug, Parser)]
#[command(
    version,
    about = "Conservative Verse formatter (community-built, not an Epic compiler)"
)]
pub struct Cli {
    /// Files or directories; use '-' alone to read UTF-8 from stdin.
    pub paths: Vec<PathBuf>,
    /// Write validated formatting changes to files.
    #[arg(long, group = "mode")]
    pub write: bool,
    /// Check formatting without modifying files; exit 1 if changes are needed.
    #[arg(long, group = "mode")]
    pub check: bool,
    /// Print a unified diff without modifying files.
    #[arg(long, group = "mode")]
    pub diff: bool,
    /// Virtual filename for stdin configuration and diagnostics, never a write target.
    #[arg(long)]
    pub stdin_filepath: Option<PathBuf>,
    /// Explicit verse.toml configuration file.
    #[arg(long)]
    pub config: Option<PathBuf>,
    /// Print the resolved configuration without processing source files.
    #[arg(long, conflicts_with_all = ["paths", "write", "check", "diff"])]
    pub show_config: bool,
    /// Explain file exclusions on stderr.
    #[arg(long)]
    pub verbose: bool,
    #[arg(long, value_enum, default_value_t = Color::Auto)]
    pub color: Color,
}

impl Cli {
    pub fn validate(&self) -> Result<(), String> {
        let stdin = self.paths.iter().any(|p| p.as_os_str() == "-");
        if stdin && self.paths.len() != 1 {
            return Err("stdin '-' cannot be combined with other paths".into());
        }
        if stdin && self.write {
            return Err("--write cannot be used with stdin".into());
        }
        if self.stdin_filepath.is_some() && !stdin && !self.show_config {
            return Err("--stdin-filepath requires stdin '-'".into());
        }
        if self.paths.is_empty() && !self.show_config {
            return Err(
                "no input; use verse-fmt <file>, or verse-fmt . --check (see --help)".into(),
            );
        }
        if !self.write
            && !self.check
            && !self.diff
            && !self.show_config
            && (self.paths.len() != 1 || self.paths[0].is_dir())
        {
            return Err(
                "directories and multiple files require --write, --check, or --diff".into(),
            );
        }
        Ok(())
    }
}
