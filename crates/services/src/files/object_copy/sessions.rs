//! Session-only secrets follow the storage lifetime; clipboard data contains IDs.
use super::*;
use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock, Weak},
};
type Entries = HashMap<String, Vec<Weak<Connection>>>;
fn entries() -> &'static Mutex<Entries> {
    static ENTRIES: OnceLock<Mutex<Entries>> = OnceLock::new();
    ENTRIES.get_or_init(Mutex::default)
}
#[derive(Default)]
pub(crate) struct Lease(Option<Arc<Connection>>);
impl Lease {
    pub fn new(connection: &Connection) -> Self {
        if !is_object(connection) || connection.text("password").is_empty() {
            return Self(None);
        }
        let connection = Arc::new(connection.clone());
        if let Ok(mut entries) = entries().lock() {
            entries
                .entry(connection.id.clone())
                .or_default()
                .push(Arc::downgrade(&connection));
        }
        Self(Some(connection))
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        if let Some(connection) = &self.0
            && let Ok(mut entries) = entries().lock()
            && let Some(leases) = entries.get_mut(&connection.id)
        {
            leases.retain(|lease| {
                !std::ptr::eq(lease.as_ptr(), Arc::as_ptr(connection)) && lease.strong_count() != 0
            });
            if leases.is_empty() {
                entries.remove(&connection.id);
            }
        }
    }
}
pub(super) fn find(id: &str) -> Option<Connection> {
    let entries = entries().lock().ok()?;
    entries
        .get(id)?
        .iter()
        .rev()
        .find_map(Weak::upgrade)
        .map(|c| (*c).clone())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn temporary_copy_credentials_are_released_with_the_last_storage_session() {
        let mut connection = Connection::new("S3");
        connection.set("password", "fixture-secret");
        connection.set("useSshTunnel", true);
        let lease = Lease::new(&connection);
        assert_eq!(
            find(&connection.id).unwrap().text("password"),
            "fixture-secret"
        );
        assert!(
            SavedConnections
                .find(&connection.id)
                .unwrap()
                .flag("useSshTunnel")
        );
        let second = Lease::new(&connection);
        drop(second);
        assert!(find(&connection.id).is_some());
        drop(lease);
        assert!(find(&connection.id).is_none());
    }
}
