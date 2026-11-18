//! `config.toml`: profiles and their servers. Never holds secrets.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use url::Url;

use crate::{Error, Result};

pub const DEFAULT_SERVER: &str = "https://hydronode.tech";
pub const DEFAULT_PROFILE: &str = "default";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Config {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_profile: Option<String>,
    #[serde(default)]
    pub profiles: BTreeMap<String, Profile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Profile {
    pub server: String,
}

/// Directory for config, credential fallback file and lock files.
///
/// `HYDRONODE_CONFIG_DIR` wins, then `$XDG_CONFIG_HOME/hydronode`, then `~/.config/hydronode`
/// (Windows: `%APPDATA%\hydronode`).
pub fn config_dir() -> Result<PathBuf> {
    if let Some(dir) = std::env::var_os("HYDRONODE_CONFIG_DIR").filter(|d| !d.is_empty()) {
        return Ok(PathBuf::from(dir));
    }
    if let Some(dir) = std::env::var_os("XDG_CONFIG_HOME").filter(|d| !d.is_empty()) {
        return Ok(PathBuf::from(dir).join("hydronode"));
    }
    let base = directories::BaseDirs::new()
        .ok_or_else(|| Error::Config("cannot find the home directory".into()))?;
    if cfg!(windows) {
        Ok(base.config_dir().join("hydronode"))
    } else {
        Ok(base.home_dir().join(".config").join("hydronode"))
    }
}

impl Config {
    pub fn path_in(dir: &Path) -> PathBuf {
        dir.join("config.toml")
    }

    /// Missing file = empty config (the built-in `default` profile still exists).
    pub fn load(path: &Path) -> Result<Config> {
        match std::fs::read_to_string(path) {
            Ok(text) => {
                toml::from_str(&text).map_err(|e| Error::Config(format!("{}: {e}", path.display())))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
            Err(e) => Err(e.into()),
        }
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = toml::to_string_pretty(self).map_err(|e| Error::Config(e.to_string()))?;
        let tmp = path.with_extension("toml.tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    /// The profile to use: the flag, else `default_profile`, else `default`.
    pub fn active_name(&self, flag: Option<&str>) -> String {
        flag.map(str::to_string)
            .or_else(|| self.default_profile.clone())
            .unwrap_or_else(|| DEFAULT_PROFILE.to_string())
    }

    /// A profile by name. `default` exists without being configured.
    pub fn profile(&self, name: &str) -> Result<Profile> {
        if let Some(p) = self.profiles.get(name) {
            return Ok(p.clone());
        }
        if name == DEFAULT_PROFILE {
            return Ok(Profile {
                server: DEFAULT_SERVER.into(),
            });
        }
        Err(Error::Usage(format!(
            "unknown profile '{name}', add it with 'hn profile add {name} --server <url>'"
        )))
    }

    /// All profiles including the implicit `default`.
    pub fn all_profiles(&self) -> BTreeMap<String, Profile> {
        let mut all = self.profiles.clone();
        all.entry(DEFAULT_PROFILE.into()).or_insert(Profile {
            server: DEFAULT_SERVER.into(),
        });
        all
    }
}

/// HTTPS only, plain HTTP only for `localhost`, `127.0.0.1` and `[::1]`. Returns the origin.
pub fn check_server(raw: &str) -> Result<Url> {
    let url = Url::parse(raw.trim())
        .map_err(|_| Error::InsecureServer(format!("'{raw}' is not a valid server URL")))?;
    if !url.username().is_empty() || url.password().is_some() {
        return Err(Error::InsecureServer(
            "the server URL must not contain a user name or password".into(),
        ));
    }
    let host = url
        .host_str()
        .ok_or_else(|| Error::InsecureServer(format!("'{raw}' has no host")))?;
    match url.scheme() {
        "https" => {}
        "http" if is_loopback(host) => {}
        "http" => {
            return Err(Error::InsecureServer(format!(
                "plain http is not allowed for {host}"
            )))
        }
        other => {
            return Err(Error::InsecureServer(format!(
                "scheme '{other}' is not supported"
            )))
        }
    }
    let origin = url.origin().ascii_serialization();
    Url::parse(&origin).map_err(|e| Error::InsecureServer(e.to_string()))
}

pub fn is_loopback(host: &str) -> bool {
    matches!(host, "localhost" | "127.0.0.1" | "[::1]" | "::1")
}
