use std::{marker::PhantomData, rc::Rc};

pub(crate) struct EngineThreadToken {
    not_send_or_sync: PhantomData<Rc<()>>,
}

impl EngineThreadToken {
    /// Creates a token while executing synchronously inside an NWServer
    /// callback.
    ///
    /// # Safety
    ///
    /// The caller must be on NWServer's active execution thread and must not
    /// retain the token after that callback returns. Event dispatch separately
    /// binds and verifies the single-threaded stack invariant used by Unified.
    pub(crate) unsafe fn new() -> Self {
        Self {
            not_send_or_sync: PhantomData,
        }
    }
}
