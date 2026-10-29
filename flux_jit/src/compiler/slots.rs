use std::collections::HashMap;

use flux_value::TaggedVal;

/// Flat slot layout — interleaved `[tag, data]` pairs per reactive node.
pub struct SlotLayout {
    pub name_to_idx: HashMap<String, u32>,
    /// Interleaved `[tag0, data0, tag1, data1, …]`; stable pointer for JIT calls.
    pub flat: Vec<u64>,
}

impl Default for SlotLayout {
    fn default() -> Self {
        Self::new()
    }
}

impl SlotLayout {
    pub fn new() -> Self {
        Self {
            name_to_idx: HashMap::new(),
            flat: Vec::new(),
        }
    }

    /// Number of node slots (each occupies two `u64` words).
    pub fn len(&self) -> usize {
        self.flat.len() / 2
    }

    pub fn is_empty(&self) -> bool {
        self.flat.is_empty()
    }

    pub fn alloc(&mut self, name: &str) -> u32 {
        let idx = self.len() as u32;
        self.name_to_idx.insert(name.to_string(), idx);
        self.flat.push(0); // tag
        self.flat.push(0); // data
        idx
    }

    pub fn write_tagged(&mut self, name: &str, tv: TaggedVal) {
        let Some(&idx) = self.name_to_idx.get(name) else {
            return;
        };
        let base = idx as usize * 2;
        if base + 1 < self.flat.len() {
            self.flat[base] = tv.tag;
            self.flat[base + 1] = tv.data;
        }
    }

    pub fn read_tagged(&self, name: &str) -> Option<TaggedVal> {
        let idx = *self.name_to_idx.get(name)? as usize;
        let base = idx * 2;
        Some(TaggedVal {
            tag: self.flat.get(base).copied().unwrap_or(0),
            data: self.flat.get(base + 1).copied().unwrap_or(0),
        })
    }

    pub fn write_bits(&mut self, name: &str, bits: u64) {
        let Some(&idx) = self.name_to_idx.get(name) else {
            return;
        };
        let base = idx as usize * 2 + 1;
        if base < self.flat.len() {
            self.flat[base] = bits;
        }
    }

    pub fn as_ptr(&self) -> *const u64 {
        self.flat.as_ptr()
    }
}
