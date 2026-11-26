use std::sync::Arc;
use std::time::Duration;

use super::database::{self, Kind};
use super::*;

fn secret(value: &str) -> Secret {
    Secret::new(value.to_owned())
}

fn cached() -> (Arc<MemoryStore>, CachedStore) {
    let memory = Arc::new(MemoryStore::new());
    let store = CachedStore::new(Arc::clone(&memory) as Arc<dyn SecretStore>);
    (memory, store)
}

#[test]
fn a_secret_is_never_printed() {
    let value = secret("hunter2");
    assert_eq!(format!("{value:?}"), "Secret(***)");
    assert_eq!(value.to_string(), "***");
    assert_eq!(format!("{:?}", Some(value.clone())), "Some(Secret(***))");
    assert_eq!(value.expose(), "hunter2");
}

#[test]
fn a_secret_deserialises_from_a_string() {
    let value: Secret = serde_json::from_value(serde_json::json!("p@ss")).unwrap();
    assert_eq!(value.expose(), "p@ss");
}

#[test]
fn ids_are_stable_and_refuse_slashes() {
    let global = database::id(None, "c1f0", Kind::Password).unwrap();
    assert_eq!((global.service.as_str(), global.account.as_str()), ("space.ifzenelse.zen.database", "c1f0/password"));
    let project = database::id(Some("shop-1a2b3c4d"), "main", Kind::Ssh).unwrap();
    assert_eq!(project.account, "shop-1a2b3c4d/main/ssh");
    assert_eq!(database::id(None, "a/b", Kind::Token), Err(SecretError::InvalidId));
    assert_eq!(database::id(None, "", Kind::Token), Err(SecretError::InvalidId));
    assert_eq!(database::id(Some("x\n"), "a", Kind::Token), Err(SecretError::InvalidId));
    assert_eq!(SecretId::new("", "a"), Err(SecretError::InvalidId));
}

#[test]
fn each_secret_is_read_once_per_session() {
    let (memory, store) = cached();
    let id = database::id(None, "c1", Kind::Password).unwrap();
    memory.set(&id, &secret("pw")).unwrap();
    let before = memory.accesses();
    for _ in 0..5 {
        assert_eq!(store.get(&id).unwrap().unwrap().expose(), "pw");
    }
    assert_eq!(memory.accesses(), before + 1);
    let other = database::id(None, "c2", Kind::Password).unwrap();
    for _ in 0..3 {
        assert!(store.get(&other).unwrap().is_none());
    }
    assert_eq!(memory.accesses(), before + 2);
}

#[test]
fn a_secret_set_through_the_cache_needs_no_read() {
    let (memory, store) = cached();
    let id = database::id(None, "c1", Kind::Token).unwrap();
    store.set(&id, &secret("t")).unwrap();
    assert_eq!(store.get(&id).unwrap().unwrap().expose(), "t");
    assert_eq!(memory.accesses(), 1);
}

#[test]
fn a_denied_access_is_reported_and_not_asked_again_until_retried() {
    let (memory, store) = cached();
    let id = database::id(None, "c1", Kind::Password).unwrap();
    memory.deny(&id);
    for _ in 0..10 {
        assert_eq!(store.get(&id).unwrap_err(), SecretError::Denied);
    }
    assert_eq!(memory.accesses(), 1);
    store.retry(&id);
    assert_eq!(store.get(&id).unwrap_err(), SecretError::Denied);
    assert_eq!(memory.accesses(), 2);
}

#[test]
fn deleting_a_connection_deletes_its_secrets_only() {
    let (memory, store) = cached();
    for kind in Kind::ALL {
        store.set(&database::id(None, "gone", kind).unwrap(), &secret("x")).unwrap();
    }
    let kept = database::id(None, "kept", Kind::Password).unwrap();
    let project = database::id(Some("p-1"), "gone", Kind::Password).unwrap();
    store.set(&kept, &secret("y")).unwrap();
    store.set(&project, &secret("z")).unwrap();
    assert_eq!(database::delete_all(&store, None, "gone"), Ok(3));
    assert_eq!(memory.ids(), vec![kept.clone(), project]);
    for kind in Kind::ALL {
        assert!(store.get(&database::id(None, "gone", kind).unwrap()).unwrap().is_none());
    }
    assert_eq!(database::delete_all(&store, Some("p-1"), "gone"), Ok(1));
    assert_eq!(memory.ids(), vec![kept]);
}

#[test]
fn deleting_tries_every_kind_and_reports_a_denial() {
    let memory = MemoryStore::new();
    let password = database::id(None, "c", Kind::Password).unwrap();
    let token = database::id(None, "c", Kind::Token).unwrap();
    memory.set(&token, &secret("t")).unwrap();
    memory.deny(&password);
    assert_eq!(database::delete_all(&memory, None, "c"), Err(SecretError::Denied));
    assert!(memory.ids().is_empty());
}

#[test]
fn an_unsupported_system_says_so() {
    let id = database::id(None, "c", Kind::Password).unwrap();
    assert_eq!(UnsupportedStore.get(&id).unwrap_err(), SecretError::Unsupported);
    assert_eq!(UnsupportedStore.set(&id, &secret("x")).unwrap_err(), SecretError::Unsupported);
    assert_eq!(UnsupportedStore.delete(&id).unwrap_err(), SecretError::Unsupported);
}

#[test]
fn whether_a_secret_is_kept_is_answered_from_what_was_read() {
    let (memory, store) = cached();
    let id = database::id(None, "c1", Kind::Password).unwrap();
    memory.set(&id, &secret("pw")).unwrap();
    assert!(store.contains(&id).unwrap(), "asked of the store while nothing is known");
    store.get(&id).unwrap();
    let before = memory.accesses();
    assert!(store.contains(&id).unwrap());
    store.delete(&id).unwrap();
    assert!(!store.contains(&id).unwrap());
    assert_eq!(memory.accesses(), before + 1, "only the delete reached the store");
}

#[test]
fn a_secret_added_outside_the_session_is_seen_once_the_absence_has_expired() {
    let memory = Arc::new(MemoryStore::new());
    let store = CachedStore::with_absent_lifetime(Arc::clone(&memory) as Arc<dyn SecretStore>, Duration::ZERO);
    let id = database::id(None, "c9", Kind::Password).unwrap();
    assert!(store.get(&id).unwrap().is_none());
    assert!(!store.contains(&id).unwrap());
    memory.set(&id, &secret("added elsewhere")).unwrap();
    assert!(store.contains(&id).unwrap());
    assert_eq!(store.get(&id).unwrap().unwrap().expose(), "added elsewhere");
}

#[test]
fn an_absence_that_has_not_expired_is_not_asked_again() {
    let (memory, store) = cached();
    let id = database::id(None, "c10", Kind::Password).unwrap();
    assert!(store.get(&id).unwrap().is_none());
    let before = memory.accesses();
    assert!(store.get(&id).unwrap().is_none());
    assert!(!store.contains(&id).unwrap());
    assert_eq!(memory.accesses(), before);
}
