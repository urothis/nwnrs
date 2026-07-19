use std::{marker::PhantomData, rc::Rc};

pub(crate) struct EngineThreadToken {
    not_send_or_sync: PhantomData<Rc<()>>,
}

impl EngineThreadToken {
    /// Creates a token while executing synchronously inside the engine's VM
    /// command callback.
    ///
    /// # Safety
    ///
    /// The caller must be the active engine VM callback and must not retain the
    /// token after that callback returns.
    pub(crate) unsafe fn new() -> Self {
        Self {
            not_send_or_sync: PhantomData,
        }
    }
}
