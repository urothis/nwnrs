use std::sync::{Arc, Mutex, OnceLock};

use nwnrs_resman::ResMan;

static SHARED_RESMAN: OnceLock<Mutex<Option<Arc<Mutex<ResMan>>>>> = OnceLock::new();

fn shared_resman_slot() -> &'static Mutex<Option<Arc<Mutex<ResMan>>>> {
    SHARED_RESMAN.get_or_init(|| Mutex::new(None))
}

pub(crate) fn set_shared_resman(resman: Arc<Mutex<ResMan>>) {
    let mut slot = match shared_resman_slot().lock() {
        Ok(slot) => slot,
        Err(error) => error.into_inner(),
    };
    *slot = Some(resman);
}

pub(crate) fn shared_resman() -> Option<Arc<Mutex<ResMan>>> {
    let slot = match shared_resman_slot().lock() {
        Ok(slot) => slot,
        Err(error) => error.into_inner(),
    };
    slot.clone()
}

#[cfg(test)]
pub(crate) fn clear_shared_resman() {
    let mut slot = match shared_resman_slot().lock() {
        Ok(slot) => slot,
        Err(error) => error.into_inner(),
    };
    *slot = None;
}
