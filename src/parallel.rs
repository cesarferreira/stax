use std::thread;

/// Shared cap for filesystem, Git, and forge I/O fan-out.
pub(crate) const IO_CONCURRENCY_LIMIT: usize = 8;

/// Apply a function in bounded batches while preserving input order.
pub(crate) fn map_ordered<T, R, F>(items: &[T], operation: F) -> Vec<R>
where
    T: Sync,
    R: Send,
    F: Fn(&T) -> R + Sync,
{
    map_ordered_with_limit(items, IO_CONCURRENCY_LIMIT, operation)
}

/// Apply a function with a caller-selected positive concurrency cap.
pub(crate) fn map_ordered_with_limit<T, R, F>(items: &[T], limit: usize, operation: F) -> Vec<R>
where
    T: Sync,
    R: Send,
    F: Fn(&T) -> R + Sync,
{
    let limit = limit.max(1);
    let mut results = Vec::with_capacity(items.len());
    thread::scope(|scope| {
        for chunk in items.chunks(limit) {
            let handles = chunk
                .iter()
                .map(|item| scope.spawn(|| operation(item)))
                .collect::<Vec<_>>();
            results.extend(
                handles
                    .into_iter()
                    .map(|handle| handle.join().expect("bounded worker panicked")),
            );
        }
    });
    results
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    #[test]
    fn map_ordered_preserves_order_and_caps_workers() {
        let active = AtomicUsize::new(0);
        let peak = AtomicUsize::new(0);
        let items = (0..(IO_CONCURRENCY_LIMIT * 3 + 1)).collect::<Vec<_>>();

        let output = map_ordered(&items, |item| {
            let current = active.fetch_add(1, Ordering::SeqCst) + 1;
            peak.fetch_max(current, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(2));
            active.fetch_sub(1, Ordering::SeqCst);
            item * 2
        });

        assert_eq!(
            output,
            items.iter().map(|item| item * 2).collect::<Vec<_>>()
        );
        assert!(peak.load(Ordering::SeqCst) <= IO_CONCURRENCY_LIMIT);
        assert!(peak.load(Ordering::SeqCst) > 1);
    }
}
