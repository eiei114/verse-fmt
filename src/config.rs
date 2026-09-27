use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::format;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Files {
    pub exclude: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct Settings {
    pub schema_version: u32,
    pub files: Files,
    pub format: format::Options,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lint: Option<toml::Table>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            schema_version: 1,
            files: Files::default(),
            format: format::Options::default(),
            lint: None,
        }
    }
}

pub struct Config {
    pub source: Option<PathBuf>,
    pub root: PathBuf,
    pub settings: Settings,
}

pub fn resolve(explicit: Option<&Path>, virtual_path: Option<&Path>) -> Result<Config, String> {
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let source = if let Some(path) = explicit {
        Some(
            std::fs::canonicalize(path)
                .map_err(|e| format!("configuration {}: {e}", path.display()))?,
        )
    } else {
        let from = virtual_path.map(|p| {
            if p.is_absolute() {
                p.to_path_buf()
            } else {
                cwd.join(p)
            }
        });
        let start = from.as_ref().and_then(|p| p.parent()).unwrap_or(&cwd);
        let mut found = None;
        for parent in start.ancestors() {
            let candidate = parent.join("verse.toml");
            if candidate
                .try_exists()
                .map_err(|e| format!("configuration {}: {e}", candidate.display()))?
            {
                found = Some(std::fs::canonicalize(&candidate).map_err(|e| e.to_string())?);
                break;
            }
            if parent
                .join(".git")
                .try_exists()
                .map_err(|e| e.to_string())?
            {
                break;
            }
        }
        found
    };
    let root = std::fs::canonicalize(source.as_deref().and_then(Path::parent).unwrap_or(&cwd))
        .map_err(|e| e.to_string())?;
    let settings = if let Some(path) = &source {
        let mut bytes = Vec::new();
        File::open(path)
            .map_err(|e| format!("configuration {}: {e}", path.display()))?
            .take(256 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() > 256 * 1024 {
            return Err("configuration exceeds 256 KiB".into());
        }
        let text = std::str::from_utf8(&bytes)
            .map_err(|_| "configuration must be UTF-8")?
            .trim_start_matches('\u{feff}');
        toml::from_str::<Settings>(text)
            .map_err(|e| format!("configuration {}: {e}", path.display()))?
    } else {
        Settings::default()
    };
    if settings.schema_version != 1 {
        return Err(format!(
            "unsupported schema-version {}; expected 1",
            settings.schema_version
        ));
    }
    // Validate globs even in --show-config mode or when no files would match.
    crate::files::excludes(&settings.files.exclude)?;
    Ok(Config {
        source,
        root,
        settings,
    })
}

impl Config {
    pub fn show(&self) -> Result<String, String> {
        let origin = self
            .source
            .as_ref()
            .map_or_else(|| "<defaults>".into(), |p| p.display().to_string());
        Ok(format!(
            "# source: {origin:?}\n# root: {:?}\n{}",
            self.root,
            toml::to_string_pretty(&self.settings).map_err(|e| e.to_string())?
        ))
    }
}
