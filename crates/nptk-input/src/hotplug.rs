/// `HotplugManager` tracks device connection state and debounces
/// connect / disconnect events so the rest of the system only hears about
/// *actual* changes, not repeat notifications.
use std::collections::HashSet;

use crate::backend::InputDeviceInfo;
use crate::backend::PhysicalDeviceId;

/// Tracks which devices are currently considered "connected".
///
/// `handle_event` returns `true` only for genuinely new connections.
#[derive(Debug, Clone)]
pub struct HotplugManager {
    connected: HashSet<PhysicalDeviceId>,
}

impl HotplugManager {
    pub fn new() -> Self {
        Self {
            connected: HashSet::new(),
        }
    }

    /// Process a device connection notification.
    ///
    /// Returns `true` if this device is genuinely new (was not previously
    /// known).  Returns `false` for duplicate / debounced-out events.
    pub fn handle_event(&mut self, event: &InputDeviceInfo) -> bool {
        self.connected.insert(event.device_id)
    }

    /// Process a device disconnection.
    ///
    /// Returns `true` if the device was actually connected before.
    pub fn handle_disconnect(&mut self, id: &PhysicalDeviceId) -> bool {
        self.connected.remove(id)
    }

    /// Returns `true` if the device is currently tracked as connected.
    pub fn is_connected(&self, id: &PhysicalDeviceId) -> bool {
        self.connected.contains(id)
    }

    /// Number of currently connected devices.
    pub fn count(&self) -> usize {
        self.connected.len()
    }

    /// Iterate over all connected device IDs.
    pub fn iter(&self) -> impl Iterator<Item = &PhysicalDeviceId> {
        self.connected.iter()
    }
}

impl Default for HotplugManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{InputBackendKind, PhysicalDeviceId};

    fn dummy_info(local_id: u64) -> InputDeviceInfo {
        InputDeviceInfo {
            device_id: PhysicalDeviceId {
                backend: InputBackendKind::Gilrs,
                local_id,
            },
            name: "test".into(),
            vendor_id: None,
            product_id: None,
            backend: InputBackendKind::Gilrs,
        }
    }

    #[test]
    fn first_connect_is_new() {
        let mut mgr = HotplugManager::new();
        assert!(mgr.handle_event(&dummy_info(1)));
        assert_eq!(mgr.count(), 1);
    }

    #[test]
    fn duplicate_connect_is_debounced() {
        let mut mgr = HotplugManager::new();
        assert!(mgr.handle_event(&dummy_info(1)));
        assert!(!mgr.handle_event(&dummy_info(1)));
        assert_eq!(mgr.count(), 1);
    }

    #[test]
    fn disconnect_and_reconnect() {
        let mut mgr = HotplugManager::new();
        let id = dummy_info(1).device_id;
        mgr.handle_event(&dummy_info(1));
        assert!(mgr.handle_disconnect(&id));
        assert_eq!(mgr.count(), 0);
        assert!(mgr.handle_event(&dummy_info(1)));
    }

    #[test]
    fn two_devices() {
        let mut mgr = HotplugManager::new();
        mgr.handle_event(&dummy_info(1));
        mgr.handle_event(&dummy_info(2));
        assert_eq!(mgr.count(), 2);
    }
}
