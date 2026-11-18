use thiserror::Error;

/// Everything that can go wrong talking to HydroNode.
///
/// The CLI maps these to exit codes: [`Error::exit_code`].
#[derive(Debug, Error)]
pub enum Error {
    /// No credential stored for the profile.
    #[error("not logged in (profile '{profile}')")]
    NotLoggedIn { profile: String },
    /// The refresh token is gone or was revoked.
    #[error("your session has ended")]
    SessionEnded,
    /// The stored credential cannot do this (read-only token).
    #[error("{0}")]
    NeedsFullLogin(String),
    /// The server answered with an error status.
    #[error("{message}")]
    Api {
        status: u16,
        code: Option<String>,
        message: String,
    },
    /// Bad input from the user (exit code 2).
    #[error("{0}")]
    Usage(String),
    /// Server URL is not allowed (plain http to a remote host, bad URL).
    #[error("{0}")]
    InsecureServer(String),
    /// Login did not complete.
    #[error("{0}")]
    Login(String),
    /// The server sent more than we are willing to read (size in MB).
    #[error("the server answer is larger than {0} MB, stopped reading")]
    TooLarge(usize),
    #[error("network error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Config(String),
    #[error("{0}")]
    Other(String),
}

pub const FULL_LOGIN_HINT: &str = "This needs a full login. Run 'hn login'.";

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    /// 0 ok, 1 error, 2 usage, 3 auth or forbidden, 4 not found.
    pub fn exit_code(&self) -> i32 {
        match self {
            Error::Usage(_) | Error::InsecureServer(_) => 2,
            Error::NotLoggedIn { .. }
            | Error::SessionEnded
            | Error::NeedsFullLogin(_)
            | Error::Login(_) => 3,
            Error::Api { status, .. } => match status {
                401 | 403 => 3,
                404 => 4,
                _ => 1,
            },
            _ => 1,
        }
    }

    /// One short line that tells the user what to do next.
    pub fn hint(&self) -> Option<String> {
        match self {
            Error::NotLoggedIn { profile } if profile == "default" => Some("run 'hn login'".into()),
            Error::NotLoggedIn { profile } => Some(format!("run 'hn -p {profile} login'")),
            Error::SessionEnded => Some("run 'hn login' to sign in again".into()),
            Error::NeedsFullLogin(_) => Some(FULL_LOGIN_HINT.into()),
            Error::InsecureServer(_) => {
                Some("use https://, plain http works only for localhost and 127.0.0.1".into())
            }
            Error::Api { status, code, .. } => match (status, code.as_deref()) {
                (_, Some("READ_TOKEN_SCOPE"))
                | (_, Some("PAT_FORBIDDEN"))
                | (_, Some("RAW_NOT_ALLOWED")) => Some(FULL_LOGIN_HINT.into()),
                (_, Some("FEATURE_OFF")) => Some("this server does not offer this yet".into()),
                (401, _) => Some("run 'hn login'".into()),
                (403, _) => Some("your account has no access to this".into()),
                (404, _) => Some("check the id, 'hn sensors list' shows yours".into()),
                (429, _) => Some("too many requests, wait a minute and try again".into()),
                _ => None,
            },
            Error::Http(_) => {
                Some("check your connection and the server ('hn profile list')".into())
            }
            _ => None,
        }
    }

    /// Short machine code for `--json` error output.
    pub fn code(&self) -> String {
        match self {
            Error::NotLoggedIn { .. } => "NOT_LOGGED_IN".into(),
            Error::SessionEnded => "SESSION_ENDED".into(),
            Error::NeedsFullLogin(_) => "NEEDS_FULL_LOGIN".into(),
            Error::Api { code: Some(c), .. } => c.clone(),
            Error::Api { status, .. } => format!("HTTP_{status}"),
            Error::Usage(_) => "USAGE".into(),
            Error::InsecureServer(_) => "INSECURE_SERVER".into(),
            Error::Login(_) => "LOGIN_FAILED".into(),
            Error::TooLarge(_) => "TOO_LARGE".into(),
            Error::Http(_) => "NETWORK".into(),
            Error::Io(_) => "IO".into(),
            Error::Config(_) => "CONFIG".into(),
            Error::Other(_) => "ERROR".into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_codes() {
        assert_eq!(Error::Usage("x".into()).exit_code(), 2);
        assert_eq!(
            Error::NotLoggedIn {
                profile: "default".into()
            }
            .exit_code(),
            3
        );
        let api = |status| Error::Api {
            status,
            code: None,
            message: "x".into(),
        };
        assert_eq!(api(403).exit_code(), 3);
        assert_eq!(api(401).exit_code(), 3);
        assert_eq!(api(404).exit_code(), 4);
        assert_eq!(api(500).exit_code(), 1);
        assert_eq!(Error::Other("x".into()).exit_code(), 1);
    }

    #[test]
    fn hint_names_profile() {
        let e = Error::NotLoggedIn {
            profile: "int".into(),
        };
        assert_eq!(e.hint().unwrap(), "run 'hn -p int login'");
    }
}
