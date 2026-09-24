use std::sync::atomic::{AtomicU64, Ordering};
use yrs::Origin;

static NEXT_SUBSCRIPTION: AtomicU64 = AtomicU64::new(1);

pub(crate) fn subscription_key() -> Origin {
    Origin::from(NEXT_SUBSCRIPTION.fetch_add(1, Ordering::Relaxed))
}

/// Dropping the Swift subscription unregisters the corresponding Yrs callback.
pub(crate) struct YSubscription {
    on_drop: Option<Box<dyn FnOnce() + Send + Sync>>,
}

impl YSubscription {
    pub(crate) fn new(on_drop: impl FnOnce() + Send + Sync + 'static) -> Self {
        Self { on_drop: Some(Box::new(on_drop)) }
    }
}

impl Drop for YSubscription {
    fn drop(&mut self) {
        if let Some(unsubscribe) = self.on_drop.take() {
            unsubscribe();
        }
    }
}
