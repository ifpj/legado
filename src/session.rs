//! Reuse protocol identities and keys; never store book or chapter responses.
use crate::api::{Account, Device, now};
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

const MAX_ALIASES: usize = 512;
const IDLE_MS: u64 = 60 * 60 * 1000;
#[derive(Clone, Default)]
pub struct Sessions(Arc<Mutex<HashMap<String, Arc<Session>>>>);
pub struct Session {
    state: Mutex<Snapshot>,
    pub content: Arc<tokio::sync::Mutex<()>>,
    touched: AtomicU64,
}
#[derive(Clone)]
pub struct Snapshot {
    pub device: Device,
    revision: u64,
}
fn identity(device: &Device, account: Option<&Account>, fallback: &str) -> String {
    let uid = account
        .map(|a| a.uid.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("0");
    if device.device_id.is_empty() {
        format!(
            "new:{fallback}:{}:{}:{uid}",
            device.device_type, device.device_brand
        )
    } else {
        format!("device:{}:{uid}", device.device_id)
    }
}
impl Sessions {
    pub fn get(&self, device: &Device, account: Option<&Account>, fallback: &str) -> Arc<Session> {
        let mut entries = self.0.lock().unwrap();
        prune(&mut entries);
        let session = entries
            .entry(identity(device, account, fallback))
            .or_insert_with(|| {
                Arc::new(Session {
                    state: Mutex::new(Snapshot {
                        device: device.clone(),
                        revision: 0,
                    }),
                    content: Arc::new(tokio::sync::Mutex::new(())),
                    touched: AtomicU64::new(now()),
                })
            })
            .clone();
        session.touched.store(now(), Ordering::Relaxed);
        session
    }
    pub fn publish(
        &self,
        session: &Arc<Session>,
        original: &Snapshot,
        device: Device,
        account: Option<&Account>,
    ) -> Device {
        let current = {
            let mut current = session.state.lock().unwrap();
            // A concurrent list/detail response must not roll back a key or
            // device that the chapter worker has just recovered.
            if current.revision == original.revision && current.device != device {
                current.device = device;
                current.revision += 1;
            }
            current.device.clone()
        };
        if !current.device_id.is_empty() {
            let mut entries = self.0.lock().unwrap();
            prune(&mut entries);
            entries.insert(identity(&current, account, ""), session.clone());
        }
        current
    }
}
impl Session {
    pub fn snapshot(&self) -> Snapshot {
        self.state.lock().unwrap().clone()
    }
}
fn prune(entries: &mut HashMap<String, Arc<Session>>) {
    let time = now();
    entries.retain(|_, session| {
        time.saturating_sub(session.touched.load(Ordering::Relaxed)) < IDLE_MS
    });
    while entries.len() >= MAX_ALIASES {
        let oldest = entries
            .iter()
            .min_by_key(|(_, s)| s.touched.load(Ordering::Relaxed))
            .map(|(k, _)| k.clone());
        if let Some(key) = oldest {
            entries.remove(&key);
        } else {
            break;
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stale_clients_and_concurrent_results_cannot_restore_old_keys() {
        let sessions = Sessions::default();
        let incoming = Device {
            device_id: "old-device".into(),
            ..Default::default()
        };
        let session = sessions.get(&incoming, None, "client");
        let before = session.snapshot();
        sessions.publish(&session, &before, incoming.clone(), None);
        let renewed = Device {
            device_id: "new-device".into(),
            key: Some("00".repeat(16)),
            key_version: 123,
            ..Default::default()
        };
        sessions.publish(&session, &before, renewed.clone(), None);
        assert_eq!(
            sessions
                .publish(&session, &before, incoming.clone(), None)
                .key_version,
            123
        );
        assert_eq!(
            sessions
                .get(&incoming, None, "client")
                .snapshot()
                .device
                .device_id,
            "new-device"
        );
        assert!(Arc::ptr_eq(
            &session,
            &sessions.get(&renewed, None, "client")
        ));
        let other = Account {
            uid: "another-user".into(),
            ..Default::default()
        };
        assert!(!Arc::ptr_eq(
            &session,
            &sessions.get(&incoming, Some(&other), "client")
        ));
    }
    #[tokio::test]
    async fn chapter_work_is_serialized_per_identity() {
        let sessions = Sessions::default();
        let device = Device {
            device_id: "same".into(),
            ..Default::default()
        };
        let session = sessions.get(&device, None, "client");
        let first = session.content.clone().lock_owned().await;
        assert!(session.content.try_lock().is_err());
        let different = sessions.get(
            &Device {
                device_id: "other".into(),
                ..Default::default()
            },
            None,
            "client",
        );
        assert!(different.content.try_lock().is_ok());
        drop(first);
        assert!(session.content.try_lock().is_ok());
    }
}
