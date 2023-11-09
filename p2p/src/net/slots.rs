use std::sync::atomic::{AtomicUsize, Ordering};

use karyons_core::async_utils::CondWait;

/// Manages available inbound and outbound slots.
pub struct ConnectionSlots {
    /// A condvar for notifying when a slot become available.
    signal: CondWait,
    /// The number of occupied slots
    slots: AtomicUsize,
    /// The maximum number of slots.
    max_slots: usize,
}

impl ConnectionSlots {
    /// Creates a new ConnectionSlots
    pub fn new(max_slots: usize) -> Self {
        Self {
            signal: CondWait::new(),
            slots: AtomicUsize::new(0),
            max_slots,
        }
    }

    /// Returns the number of occupied slots
    pub fn load(&self) -> usize {
        self.slots.load(Ordering::SeqCst)
    }

    /// Increases the occupied slots by one.
    pub fn add(&self) {
        self.slots.fetch_add(1, Ordering::SeqCst);
    }

    /// Decreases the occupied slots by one and notifies the waiting signal
    /// to start accepting/connecting new connections.
    pub async fn remove(&self) {
        self.slots.fetch_sub(1, Ordering::SeqCst);
        if self.slots.load(Ordering::SeqCst) < self.max_slots {
            self.signal.signal().await;
        }
    }

    /// Waits for a slot to become available.
    pub async fn wait_for_slot(&self) {
        if self.slots.load(Ordering::SeqCst) < self.max_slots {
            return;
        }

        // Wait for a signal
        self.signal.wait().await;
        self.signal.reset().await;
    }
}
