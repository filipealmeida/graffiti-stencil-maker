//! Data parallelism that exists only where threads do: native builds, and the browser build with the `threads`
//! feature (a worker pool). Elsewhere the same calls run serially.

/// Runs the closure over every item.
pub fn each_mut<T: Send, F: Fn(&mut T) + Sync + Send>(items: &mut [T], f: F) {
    #[cfg(any(not(target_arch = "wasm32"), feature = "threads"))]
    {
        use rayon::prelude::*;
        items.par_iter_mut().for_each(f);
    }
    #[cfg(all(target_arch = "wasm32", not(feature = "threads")))]
    items.iter_mut().for_each(f);
}

/// Maps every item, keeping the order.
pub fn map<T: Sync, R: Send, F: Fn(&T) -> R + Sync + Send>(items: &[T], f: F) -> Vec<R> {
    #[cfg(any(not(target_arch = "wasm32"), feature = "threads"))]
    {
        use rayon::prelude::*;
        items.par_iter().map(f).collect()
    }
    #[cfg(all(target_arch = "wasm32", not(feature = "threads")))]
    items.iter().map(f).collect()
}
