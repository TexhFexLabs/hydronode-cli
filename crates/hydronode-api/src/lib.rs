//! Client library behind the HydroNode CLI: API client, login, credential store, device signing.

pub mod auth;
pub mod client;
pub mod config;
pub mod credentials;
pub mod device;
pub mod error;
pub mod time;

pub use error::{Error, Result};
