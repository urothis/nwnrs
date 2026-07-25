use std::{
    collections::{BTreeMap, VecDeque},
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

use nwnrs_runtime::{EventObjectId, EventValue};

use crate::bridge::BridgeInstallError;

pub(crate) const DEFAULT_EVENT_PUMP_CAPACITY: usize = 1_024;
pub(crate) const DEFAULT_EVENT_PUMP_DRAIN_LIMIT: usize = 256;

/// One producer-owned asynchronous event ready for engine-thread dispatch.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct EventPumpRecord {
    pub(crate) sequence: u64,
    pub(crate) identity: String,
    pub(crate) target:   EventObjectId,
    pub(crate) data:     BTreeMap<String, EventValue>,
}

struct EventPumpState {
    records: VecDeque<EventPumpRecord>,
}

/// Bounded multi-producer handoff drained only from the NWServer main loop.
pub(crate) struct EventPump {
    capacity:         usize,
    next_sequence:    AtomicU64,
    overflow_count:   AtomicU64,
    accepting_events: AtomicBool,
    state:            Mutex<EventPumpState>,
}

impl Default for EventPump {
    fn default() -> Self {
        Self::with_capacity(DEFAULT_EVENT_PUMP_CAPACITY)
    }
}

impl EventPump {
    pub(crate) fn with_capacity(capacity: usize) -> Self {
        Self {
            capacity,
            next_sequence: AtomicU64::new(1),
            overflow_count: AtomicU64::new(0),
            accepting_events: AtomicBool::new(true),
            state: Mutex::new(EventPumpState {
                records: VecDeque::with_capacity(capacity),
            }),
        }
    }

    /// Queues an owned record without retaining engine pointers.
    #[allow(dead_code)]
    pub(crate) fn enqueue(
        &self,
        identity: impl Into<String>,
        target: EventObjectId,
        data: BTreeMap<String, EventValue>,
    ) -> Result<u64, BridgeInstallError> {
        if !self.accepting_events.load(Ordering::Acquire) {
            return Err(BridgeInstallError::new("event pump is closed for shutdown"));
        }
        let sequence = self.next_sequence.fetch_add(1, Ordering::Relaxed);
        if sequence == u64::MAX {
            return Err(BridgeInstallError::new(
                "event pump sequence space was exhausted",
            ));
        }
        let mut state = self
            .state
            .lock()
            .map_err(|_error| BridgeInstallError::new("event pump lock is poisoned"))?;
        if state.records.len() >= self.capacity {
            self.overflow_count.fetch_add(1, Ordering::Relaxed);
            return Err(BridgeInstallError::new(format!(
                "event pump capacity {} was exceeded",
                self.capacity
            )));
        }
        state.records.push_back(EventPumpRecord {
            sequence,
            identity: identity.into(),
            target,
            data,
        });
        Ok(sequence)
    }

    /// Removes at most `limit` records in producer sequence order.
    pub(crate) fn drain(&self, limit: usize) -> Result<Vec<EventPumpRecord>, BridgeInstallError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_error| BridgeInstallError::new("event pump lock is poisoned"))?;
        let count = limit.min(state.records.len());
        Ok(state.records.drain(..count).collect())
    }

    /// Clears queued work and accepts events for the newly loaded module.
    pub(crate) fn reset(&self) -> Result<usize, BridgeInstallError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_error| BridgeInstallError::new("event pump lock is poisoned"))?;
        let discarded = state.records.len();
        state.records.clear();
        self.accepting_events.store(true, Ordering::Release);
        Ok(discarded)
    }

    /// Stops producers and discards queued records during server shutdown.
    pub(crate) fn close(&self) -> Result<usize, BridgeInstallError> {
        self.accepting_events.store(false, Ordering::Release);
        let mut state = self
            .state
            .lock()
            .map_err(|_error| BridgeInstallError::new("event pump lock is poisoned"))?;
        let discarded = state.records.len();
        state.records.clear();
        Ok(discarded)
    }

    #[allow(dead_code)]
    pub(crate) fn overflow_count(&self) -> u64 {
        self.overflow_count.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(value: u32) -> EventObjectId {
        EventObjectId::new(value)
    }

    #[test]
    fn pump_is_bounded_and_preserves_sequence_order() -> Result<(), BridgeInstallError> {
        let pump = EventPump::with_capacity(2);
        let first = pump.enqueue("resource.added", target(1), BTreeMap::new())?;
        let second = pump.enqueue("resource.modified", target(2), BTreeMap::new())?;
        assert!(second > first);
        assert!(
            pump.enqueue("resource.removed", target(3), BTreeMap::new())
                .expect_err("full queue must fail")
                .to_string()
                .contains("capacity")
        );
        assert_eq!(pump.overflow_count(), 1);
        let records = pump.drain(1)?;
        assert_eq!(records.len(), 1);
        assert_eq!(records.first().map(|record| record.sequence), Some(first));
        let records = pump.drain(8)?;
        assert_eq!(records.len(), 1);
        assert_eq!(records.first().map(|record| record.sequence), Some(second));
        Ok(())
    }

    #[test]
    fn shutdown_closes_producers_and_reload_reopens_them() -> Result<(), BridgeInstallError> {
        let pump = EventPump::with_capacity(2);
        let _sequence = pump.enqueue("resource.added", target(1), BTreeMap::new())?;
        assert_eq!(pump.close()?, 1);
        assert!(
            pump.enqueue("resource.added", target(1), BTreeMap::new())
                .is_err()
        );
        assert_eq!(pump.reset()?, 0);
        let _sequence = pump.enqueue("resource.added", target(1), BTreeMap::new())?;
        Ok(())
    }
}
