use apple_native_keyring_store::keychain::{Cred, MacKeychainDomain};
use keyring_core::Entry;
use keyring_core::error::Error;
use security_framework::item::{ItemClass, ItemSearchOptions};
use zeroize::Zeroize;

use crate::{Secret, SecretError, SecretId, SecretStore};

const USER_CANCELED: i32 = -128;
const AUTH_FAILED: i32 = -25293;
const INTERACTION_NOT_ALLOWED: i32 = -25308;
const ITEM_NOT_FOUND: i32 = -25300;

pub(crate) struct KeychainStore;

fn entry(id: &SecretId) -> Result<Entry, SecretError> {
    Cred::build(MacKeychainDomain::User, &id.service, &id.account).map_err(|_| SecretError::InvalidId)
}

fn reason(error: Error) -> SecretError {
    match error {
        Error::NoStorageAccess(_) => SecretError::Denied,
        Error::PlatformFailure(platform) => {
            let code = platform.downcast_ref::<security_framework::base::Error>().map(|error| error.code());
            match code {
                Some(USER_CANCELED | AUTH_FAILED | INTERACTION_NOT_ALLOWED) => SecretError::Denied,
                _ => SecretError::Failed(platform.to_string()),
            }
        }
        Error::BadEncoding(mut bytes) | Error::BadDataFormat(mut bytes, _) => {
            bytes.zeroize();
            SecretError::Failed("the item is not text".to_owned())
        }
        Error::Invalid(..) | Error::TooLong(..) => SecretError::InvalidId,
        Error::Ambiguous(_) => SecretError::Failed("more than one item matches".to_owned()),
        other => SecretError::Failed(other.to_string()),
    }
}

impl SecretStore for KeychainStore {
    fn get(&self, id: &SecretId) -> Result<Option<Secret>, SecretError> {
        match entry(id)?.get_secret() {
            Ok(bytes) => match String::from_utf8(bytes) {
                Ok(text) => Ok(Some(Secret::new(text))),
                Err(error) => {
                    error.into_bytes().zeroize();
                    Err(SecretError::Failed("the item is not text".to_owned()))
                }
            },
            Err(Error::NoEntry) => Ok(None),
            Err(error) => Err(reason(error)),
        }
    }

    fn set(&self, id: &SecretId, secret: &Secret) -> Result<(), SecretError> {
        entry(id)?.set_secret(secret.expose().as_bytes()).map_err(reason)
    }

    fn contains(&self, id: &SecretId) -> Result<bool, SecretError> {
        let mut search = ItemSearchOptions::new();
        search
            .class(ItemClass::generic_password())
            .service(&id.service)
            .account(&id.account)
            .load_attributes(true)
            .limit(1);
        match search.search() {
            Ok(found) => Ok(!found.is_empty()),
            Err(error) if error.code() == ITEM_NOT_FOUND => Ok(false),
            Err(error) => match error.code() {
                USER_CANCELED | AUTH_FAILED | INTERACTION_NOT_ALLOWED => Err(SecretError::Denied),
                _ => Err(SecretError::Failed(error.to_string())),
            },
        }
    }

    fn delete(&self, id: &SecretId) -> Result<bool, SecretError> {
        match entry(id)?.delete_credential() {
            Ok(()) => Ok(true),
            Err(Error::NoEntry) => Ok(false),
            Err(error) => Err(reason(error)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "touches the login Keychain and may show the system's prompt; run by hand only"]
    fn a_secret_goes_into_the_keychain_and_out_again() {
        let id = SecretId::new("space.ifzenelse.zen.test", "zen-secrets/ignored-test").unwrap();
        let store = KeychainStore;
        store.set(&id, &Secret::new("s3cret".to_owned())).unwrap();
        assert_eq!(store.get(&id).unwrap().unwrap().expose(), "s3cret");
        assert!(store.contains(&id).unwrap());
        assert!(store.delete(&id).unwrap());
        assert!(store.get(&id).unwrap().is_none());
        assert!(!store.contains(&id).unwrap());
        assert!(!store.delete(&id).unwrap());
    }
}
