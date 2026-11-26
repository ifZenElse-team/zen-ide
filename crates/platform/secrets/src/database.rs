use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{SecretError, SecretId, SecretStore};

pub const SERVICE: &str = "space.ifzenelse.zen.database";

/// The kind of a connection's secret.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    /// The database password.
    Password,
    /// The SSH password or key passphrase.
    Ssh,
    /// An API key or token.
    Token,
}

impl Kind {
    pub const ALL: [Kind; 3] = [Kind::Password, Kind::Ssh, Kind::Token];

    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Password => "password",
            Kind::Ssh => "ssh",
            Kind::Token => "token",
        }
    }
}

fn plain(part: &str) -> bool {
    !part.is_empty() && !part.contains('/') && !part.chars().any(char::is_control)
}

pub fn id(project: Option<&str>, connection: &str, kind: Kind) -> Result<SecretId, SecretError> {
    if !plain(connection) || project.is_some_and(|project| !plain(project)) {
        return Err(SecretError::InvalidId);
    }
    let account = match project {
        Some(project) => format!("{project}/{connection}/{}", kind.as_str()),
        None => format!("{connection}/{}", kind.as_str()),
    };
    SecretId::new(SERVICE, account)
}

pub fn delete_all(store: &dyn SecretStore, project: Option<&str>, connection: &str) -> Result<usize, SecretError> {
    let mut deleted = 0;
    let mut failed = None;
    for kind in Kind::ALL {
        match store.delete(&id(project, connection, kind)?) {
            Ok(true) => deleted += 1,
            Ok(false) => {}
            Err(error) => {
                failed.get_or_insert(error);
            }
        }
    }
    failed.map_or(Ok(deleted), Err)
}
