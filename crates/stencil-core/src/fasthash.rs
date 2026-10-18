//! A small multiplicative hasher for integer keys. The standard SipHash is built to resist hostile keys, which
//! these geometry grids never see, and it dominates the time of hashing millions of vertices.
use std::collections::{HashMap, HashSet};
use std::hash::{BuildHasherDefault, Hasher};

const K: u64 = 0x517c_c1b7_2722_0a95;

#[derive(Default, Clone, Copy)]
pub struct Fx(u64);

impl Fx {
    fn add(&mut self, v: u64) {
        self.0 = (self.0.rotate_left(5) ^ v).wrapping_mul(K);
    }
}

impl Hasher for Fx {
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.add(b as u64);
        }
    }
    fn write_u32(&mut self, i: u32) {
        self.add(i as u64);
    }
    fn write_i32(&mut self, i: i32) {
        self.add(i as u32 as u64);
    }
    fn write_u64(&mut self, i: u64) {
        self.add(i);
    }
    fn write_i64(&mut self, i: i64) {
        self.add(i as u64);
    }
    fn write_usize(&mut self, i: usize) {
        self.add(i as u64);
    }
    fn finish(&self) -> u64 {
        self.0
    }
}

pub type FastMap<K, V> = HashMap<K, V, BuildHasherDefault<Fx>>;
pub type FastSet<K> = HashSet<K, BuildHasherDefault<Fx>>;
