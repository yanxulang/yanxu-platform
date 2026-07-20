//! 共享平台状态的中毒锁恢复与可观测计数。

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};

static RECOVERED_LOCKS: AtomicU64 = AtomicU64::new(0);

/// 为平台内部互斥锁提供一致的中毒恢复行为。
pub trait RecoverMutex<T> {
    /// 取得锁；若先前持有者发生恐慌，则保留状态、清除中毒标记并记录一次恢复。
    fn lock_recover(&self) -> MutexGuard<'_, T>;
}

impl<T> RecoverMutex<T> for Mutex<T> {
    fn lock_recover(&self) -> MutexGuard<'_, T> {
        match self.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                RECOVERED_LOCKS.fetch_add(1, Ordering::Relaxed);
                self.clear_poison();
                poisoned.into_inner()
            }
        }
    }
}

/// 返回本进程内已经恢复的中毒平台锁次数。
#[must_use]
pub fn recovered_lock_count() -> u64 {
    RECOVERED_LOCKS.load(Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn poisoned_mutex_is_recovered_and_counted() {
        let state = Arc::new(Mutex::new(7));
        let poisoned = Arc::clone(&state);
        let before = recovered_lock_count();
        assert!(
            std::thread::spawn(move || {
                let _guard = poisoned.lock().unwrap();
                panic!("fault injection");
            })
            .join()
            .is_err()
        );

        assert_eq!(*state.lock_recover(), 7);
        assert!(!state.is_poisoned());
        assert!(recovered_lock_count() > before);
    }
}
