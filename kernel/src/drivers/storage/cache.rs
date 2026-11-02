// kernel/src/drivers/storage/cache.rs — Page Cache / Block Buffer Cache para VFS
// Proporciona caché de sectores en RAM acelerado por el asignador Buddy System.

#![allow(dead_code)]

use crate::arch::Spinlock;

pub const CACHE_LINES: usize = 64;
pub const SECTOR_SIZE: usize = 512;

#[derive(Clone, Copy)]
pub struct CacheEntry {
    pub lba: u64,
    pub valid: bool,
    pub dirty: bool,
    pub access_counter: u64,
    pub data: [u8; SECTOR_SIZE],
}

impl CacheEntry {
    pub const fn empty() -> Self {
        Self {
            lba: 0,
            valid: false,
            dirty: false,
            access_counter: 0,
            data: [0u8; SECTOR_SIZE],
        }
    }
}

pub struct PageCache {
    entries: [CacheEntry; CACHE_LINES],
    access_clock: u64,
    hits: u64,
    misses: u64,
}

impl PageCache {
    pub const fn new() -> Self {
        Self {
            entries: [CacheEntry::empty(); CACHE_LINES],
            access_clock: 0,
            hits: 0,
            misses: 0,
        }
    }

    pub fn lookup(&mut self, lba: u64, out: &mut [u8; SECTOR_SIZE]) -> bool {
        self.access_clock = self.access_clock.wrapping_add(1);
        for entry in self.entries.iter_mut() {
            if entry.valid && entry.lba == lba {
                entry.access_counter = self.access_clock;
                out.copy_from_slice(&entry.data);
                self.hits = self.hits.saturating_add(1);
                return true;
            }
        }
        self.misses = self.misses.saturating_add(1);
        false
    }

    pub fn insert(&mut self, lba: u64, data: &[u8; SECTOR_SIZE]) {
        self.access_clock = self.access_clock.wrapping_add(1);
        // Buscar si ya existe
        for entry in self.entries.iter_mut() {
            if entry.valid && entry.lba == lba {
                entry.access_counter = self.access_clock;
                entry.data.copy_from_slice(data);
                entry.dirty = false;
                return;
            }
        }

        // Buscar entrada libre
        for entry in self.entries.iter_mut() {
            if !entry.valid {
                entry.valid = true;
                entry.lba = lba;
                entry.access_counter = self.access_clock;
                entry.data.copy_from_slice(data);
                entry.dirty = false;
                return;
            }
        }

        // Reemplazo LRU
        let mut oldest_idx = 0;
        let mut min_acc = u64::MAX;
        for (i, entry) in self.entries.iter().enumerate() {
            if entry.access_counter < min_acc {
                min_acc = entry.access_counter;
                oldest_idx = i;
            }
        }

        self.entries[oldest_idx].valid = true;
        self.entries[oldest_idx].lba = lba;
        self.entries[oldest_idx].access_counter = self.access_clock;
        self.entries[oldest_idx].data.copy_from_slice(data);
        self.entries[oldest_idx].dirty = false;
    }

    pub fn invalidate(&mut self, lba: u64) {
        for entry in self.entries.iter_mut() {
            if entry.valid && entry.lba == lba {
                entry.valid = false;
                break;
            }
        }
    }

    pub fn stats(&self) -> (u64, u64, u8) {
        let total = self.hits + self.misses;
        let ratio = if total > 0 {
            ((self.hits * 100) / total) as u8
        } else {
            100
        };
        (self.hits, self.misses, ratio)
    }
}

pub static PAGE_CACHE: Spinlock<PageCache> = Spinlock::new(PageCache::new());

pub fn cache_read_sector(lba: u64, out: &mut [u8; SECTOR_SIZE]) -> bool {
    PAGE_CACHE.lock().lookup(lba, out)
}

pub fn cache_write_sector(lba: u64, data: &[u8; SECTOR_SIZE]) {
    PAGE_CACHE.lock().insert(lba, data);
}

pub fn cache_invalidate_sector(lba: u64) {
    PAGE_CACHE.lock().invalidate(lba);
}

pub fn cache_get_stats() -> (u64, u64, u8) {
    PAGE_CACHE.lock().stats()
}
