// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Crisp IM SAS
// License: Mozilla Public License v2.0 (MPL v2.0)

use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::directory::BackendStatus;
use crate::error::{RouterError, RouterResult};

const DEFAULTS: &str = r#"
[server]
log_level = "error"

[channel]
inet = "[::1]:1490"
tcp_timeout = 300
bulk_buffer_size = 8388608

[admin]
inet = "[::1]:1492"

[directory]
path = "./data/router/directory.db"
"#;

#[derive(Clone, Deserialize)]
pub struct Config {
    pub server: ServerConfig,
    pub channel: ChannelConfig,
    pub admin: AdminConfig,
    pub directory: DirectoryConfig,
    #[serde(default, alias = "backends")]
    pub servers: Vec<BackendConfig>,
}

#[derive(Clone, Deserialize)]
pub struct ServerConfig {
    pub log_level: String,
}

#[derive(Clone, Deserialize)]
pub struct ChannelConfig {
    pub inet: SocketAddr,
    pub tcp_timeout: u64,
    pub bulk_buffer_size: usize,
    #[serde(default)]
    pub auth_password: Option<String>,
}

#[derive(Clone, Deserialize)]
pub struct AdminConfig {
    pub inet: SocketAddr,
    #[serde(default)]
    pub auth_password: Option<String>,
}

#[derive(Clone, Deserialize)]
pub struct DirectoryConfig {
    pub path: PathBuf,
}

#[derive(Clone, Deserialize, Eq, PartialEq)]
pub struct BackendConfig {
    pub id: String,
    pub address: String,
    #[serde(default)]
    pub auth_password: String,
    #[serde(default)]
    pub status: BackendStatus,
    #[serde(default = "default_weight", alias = "capacity")]
    pub weight: u32,
}

const fn default_weight() -> u32 {
    1
}

impl Config {
    pub fn read(path: &Path) -> RouterResult<Self> {
        let mut builder = config::Config::builder()
            .add_source(config::File::from_str(DEFAULTS, config::FileFormat::Toml));

        if path.exists() {
            builder = builder.add_source(
                config::File::from(path)
                    .format(config::FileFormat::Toml)
                    .required(true),
            );
        }

        let parsed = builder
            .add_source(
                config::Environment::with_prefix("SONIC_ROUTER")
                    .separator("__")
                    .prefix_separator("_"),
            )
            .build()?
            .try_deserialize::<Self>()?;

        parsed.validate()?;

        Ok(parsed)
    }

    fn validate(&self) -> RouterResult<()> {
        if self.channel.tcp_timeout == 0 {
            return Err(RouterError::code("channel.tcp_timeout must be positive"));
        }

        if self.channel.bulk_buffer_size < 20_000 {
            return Err(RouterError::code(
                "channel.bulk_buffer_size must be at least 20000",
            ));
        }

        for backend in &self.servers {
            if backend.id.is_empty() {
                return Err(RouterError::code("server id must not be empty"));
            }
            if backend.address.is_empty() {
                return Err(RouterError::code("server address must not be empty"));
            }
            if backend.weight == 0 {
                return Err(RouterError::code("server weight must be positive"));
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_defaults_without_file() {
        let config = Config::read(Path::new("/path/which/does/not/exist")).unwrap();

        assert_eq!(config.channel.inet.port(), 1490);
        assert_eq!(config.admin.inet.port(), 1492);
        assert_eq!(
            config.directory.path,
            PathBuf::from("./data/router/directory.db")
        );
    }
}
