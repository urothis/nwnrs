use std::{
    any::Any,
    collections::BTreeMap,
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

use crate::bridge::BridgeInstallError;

/// Monotonic identity assigned to one cross-hook correlation record.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct CorrelationToken(u64);

impl CorrelationToken {
    #[cfg(test)]
    const fn raw(self) -> u64 {
        self.0
    }
}

struct CorrelationRecord {
    token: CorrelationToken,
    value: Box<dyn Any + Send>,
}

type CorrelationKey = (&'static str, u64);

/// Owns state which begins in one physical hook and finishes in another.
///
/// Values must be owned and `Send`, which prevents event families from
/// retaining borrowed engine objects across hook boundaries. Records are LIFO
/// within a declared owner/key pair.
pub(crate) struct CorrelationOwner {
    next_token: AtomicU64,
    records:    Mutex<BTreeMap<CorrelationKey, Vec<CorrelationRecord>>>,
}

impl Default for CorrelationOwner {
    fn default() -> Self {
        Self {
            next_token: AtomicU64::new(1),
            records:    Mutex::new(BTreeMap::new()),
        }
    }
}

impl CorrelationOwner {
    /// Begins one correlated operation and returns its monotonic token.
    #[allow(dead_code)]
    pub(crate) fn begin<Value>(
        &self,
        owner: &'static str,
        key: u64,
        value: Value,
    ) -> Result<CorrelationToken, BridgeInstallError>
    where
        Value: Any + Send,
    {
        if owner.is_empty() {
            return Err(BridgeInstallError::new(
                "correlation owner name cannot be empty",
            ));
        }
        let token = self.next_token.fetch_add(1, Ordering::Relaxed);
        if token == u64::MAX {
            return Err(BridgeInstallError::new(
                "event correlation token space was exhausted",
            ));
        }
        let token = CorrelationToken(token);
        self.records
            .lock()
            .map_err(|_error| BridgeInstallError::new("event correlation lock is poisoned"))?
            .entry((owner, key))
            .or_default()
            .push(CorrelationRecord {
                token,
                value: Box::new(value),
            });
        Ok(token)
    }

    /// Finishes the most recent record for an owner/key pair.
    ///
    /// Supplying the token makes duplicate and out-of-order completion
    /// observable instead of silently pairing the wrong native operations.
    #[allow(dead_code)]
    pub(crate) fn finish<Value>(
        &self,
        owner: &'static str,
        key: u64,
        token: CorrelationToken,
    ) -> Result<Value, BridgeInstallError>
    where
        Value: Any + Send,
    {
        let mut records = self
            .records
            .lock()
            .map_err(|_error| BridgeInstallError::new("event correlation lock is poisoned"))?;
        let correlation_key = (owner, key);
        let stack = records.get_mut(&correlation_key).ok_or_else(|| {
            BridgeInstallError::new(format!(
                "event correlation {owner}/{key} finished without a matching begin"
            ))
        })?;
        let actual = stack.last().map(|record| record.token).ok_or_else(|| {
            BridgeInstallError::new(format!(
                "event correlation {owner}/{key} has an empty record stack"
            ))
        })?;
        if actual != token {
            stack.clear();
            records.remove(&correlation_key);
            return Err(BridgeInstallError::new(format!(
                "event correlation {owner}/{key} finished out of order"
            )));
        }
        let record = stack.pop().ok_or_else(|| {
            BridgeInstallError::new("event correlation record disappeared before completion")
        })?;
        if stack.is_empty() {
            records.remove(&correlation_key);
        }
        record
            .value
            .downcast::<Value>()
            .map(|value| *value)
            .map_err(|_value| {
                BridgeInstallError::new(format!(
                    "event correlation {owner}/{key} completed with the wrong owned value type"
                ))
            })
    }

    /// Discards all outstanding records during module reload or shutdown.
    pub(crate) fn clear(&self) -> Result<usize, BridgeInstallError> {
        let mut records = self
            .records
            .lock()
            .map_err(|_error| BridgeInstallError::new("event correlation lock is poisoned"))?;
        let discarded = records.values().map(Vec::len).sum();
        records.clear();
        Ok(discarded)
    }

    #[cfg(test)]
    fn outstanding(&self) -> usize {
        self.records
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .values()
            .map(Vec::len)
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn correlations_are_monotonic_and_lifo_per_key() -> Result<(), BridgeInstallError> {
        let owner = CorrelationOwner::default();
        let first = owner.begin("combat.attack", 7, String::from("first"))?;
        let second = owner.begin("combat.attack", 7, String::from("second"))?;
        assert!(second.raw() > first.raw());
        assert_eq!(
            owner.finish::<String>("combat.attack", 7, second)?,
            "second"
        );
        assert_eq!(owner.finish::<String>("combat.attack", 7, first)?, "first");
        assert_eq!(owner.outstanding(), 0);
        Ok(())
    }

    #[test]
    fn out_of_order_completion_discards_the_affected_stack() -> Result<(), BridgeInstallError> {
        let owner = CorrelationOwner::default();
        let first = owner.begin("movement", 3, 10_u32)?;
        let _second = owner.begin("movement", 3, 20_u32)?;
        let error = owner
            .finish::<u32>("movement", 3, first)
            .expect_err("out-of-order completion must fail");
        assert!(error.to_string().contains("out of order"));
        assert_eq!(owner.outstanding(), 0);
        assert!(
            owner
                .finish::<u32>("movement", 3, first)
                .expect_err("duplicate completion must fail")
                .to_string()
                .contains("without a matching begin")
        );
        Ok(())
    }

    #[test]
    fn clear_discards_every_owned_record() -> Result<(), BridgeInstallError> {
        let owner = CorrelationOwner::default();
        let _first = owner.begin("one", 1, vec![1_u8])?;
        let _second = owner.begin("two", 2, vec![2_u8])?;
        assert_eq!(owner.clear()?, 2);
        assert_eq!(owner.outstanding(), 0);
        Ok(())
    }
}
