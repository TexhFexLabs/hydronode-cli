//! What every command gets: config, active profile, output settings, the API client.

use std::io::IsTerminal;
use std::path::PathBuf;

use hydronode_api::client::Client;
use hydronode_api::config::{check_server, config_dir, Config};
use hydronode_api::credentials::{CredentialStore, StoreKind};
use hydronode_api::{Error, Result};
use url::Url;

use crate::output::Out;

pub struct Ctx {
    pub dir: PathBuf,
    pub config: Config,
    pub profile: String,
    pub out: Out,
    pub bin: String,
}

impl Ctx {
    pub fn load(profile_flag: Option<&str>, json: bool, bin: &str) -> Result<Ctx> {
        let dir = config_dir()?;
        let config = Config::load(&Config::path_in(&dir))?;
        let profile = config.active_name(profile_flag);
        Ok(Ctx {
            dir,
            config,
            profile,
            out: Out::new(json),
            bin: bin.to_string(),
        })
    }

    pub fn config_path(&self) -> PathBuf {
        Config::path_in(&self.dir)
    }

    pub fn save_config(&self) -> Result<()> {
        self.config.save(&self.config_path())
    }

    pub fn server(&self) -> Result<Url> {
        check_server(&self.config.profile(&self.profile)?.server)
    }

    pub fn store(&self) -> CredentialStore {
        CredentialStore::new(&self.dir, StoreKind::from_env())
    }

    pub fn client(&self) -> Result<Client> {
        Ok(Client::new(self.server()?, &self.profile, self.store()))
    }

    /// `hn` or `hn -p work`, for hints.
    pub fn hn(&self) -> String {
        if self.profile == "default" {
            "hn".into()
        } else {
            format!("hn -p {}", self.profile)
        }
    }
}

pub fn stdin_is_tty() -> bool {
    std::io::stdin().is_terminal()
}

/// Ask before something destructive. `--yes` skips it; without a terminal it is a usage error.
pub fn confirm(question: &str, yes: bool) -> Result<bool> {
    if yes {
        return Ok(true);
    }
    if !stdin_is_tty() {
        return Err(Error::Usage(format!(
            "{question} Add --yes to confirm without a prompt."
        )));
    }
    dialoguer::Confirm::new()
        .with_prompt(question)
        .default(false)
        .interact()
        .map_err(|e| Error::Other(e.to_string()))
}
