//! Stable cache keys and access to retained and freshly computed outputs.

use super::{types::Outputs, Engine, Value};
use std::rc::Rc;

pub(super) const CACHE_LIMIT: usize = 64;

/// FNV-1a over length-prefixed parts. Unlike `DefaultHasher` it gives the same id on 32-bit wasm and 64-bit native,
/// so a project saved in the browser finds its pictures again in the CLI.
pub(super) fn hash_hex(parts: &[&[u8]]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut eat = |bytes: &[u8]| {
        for &b in bytes {
            h = (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01b3);
        }
    };
    for p in parts {
        eat(&(p.len() as u64).to_le_bytes());
        eat(p);
    }
    format!("{h:016x}")
}

impl Engine {
    pub(super) fn cache_outputs(&mut self, key: String, vals: Rc<Outputs>) {
        for (n, _) in vals.iter() {
            self.fresh.push((key.clone(), n.to_string()));
        }
        self.cache.insert(key.clone(), vals);
        self.order.push_back(key);
        while self.order.len() > CACHE_LIMIT {
            if let Some(old) = self.order.pop_front() {
                self.cache.remove(&old);
            }
        }
    }

    pub fn value(&self, key: &str, out: &str) -> Option<&Value> {
        self.cache
            .get(key)?
            .iter()
            .find(|(n, _)| *n == out)
            .map(|(_, v)| v)
    }

    /// Outputs computed by the last run, as `(key, output)`.
    pub fn take_fresh(&mut self) -> Vec<(String, String)> {
        std::mem::take(&mut self.fresh)
    }
}
