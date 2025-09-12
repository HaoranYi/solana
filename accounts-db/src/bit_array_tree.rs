//! Nested bit array tree for efficient liquidity tracking
//!
//! This module implements a three-level hierarchical bit array structure optimized
//! for finding populated liquidity bins across large price ranges. The structure
//! provides near-constant-time lookups and efficient jump-ahead functionality.

use std::ops::{BitAnd, BitOrAssign};

/// A hierarchical tree of nested 256-bit arrays for efficient liquidity tracking.
/// 
/// The tree consists of three levels:
/// - Top-level: Each bit represents a large range of price ticks (65536 ticks)
/// - Mid-level: Each bit represents a medium range of price ticks (256 ticks)  
/// - Low-level: Each bit represents a single liquidity bin
///
/// This structure allows for extremely fast lookups of available liquidity and
/// efficient "jump-ahead" operations to find the next populated bin.
#[derive(Clone, Debug, PartialEq)]
pub struct BitArrayTree {
    /// Top-level bit array: each bit covers 256 * 256 = 65536 bins
    top_level: [u64; 4], // 256 bits = 4 * 64-bit words
    
    /// Mid-level bit arrays: 256 arrays of 256 bits each
    mid_level: [[u64; 4]; 256],
    
    /// Low-level bit arrays: 65536 arrays of 256 bits each (on-demand allocation)
    /// Using Box to reduce stack size and enable lazy allocation
    low_level: Box<[Option<Box<[u64; 4]>>; 65536]>,
    
    /// Count of set bits for efficient len() operations
    bit_count: usize,
}

impl Default for BitArrayTree {
    fn default() -> Self {
        Self::new()
    }
}

impl BitArrayTree {
    /// Create a new empty bit array tree
    pub fn new() -> Self {
        Self {
            top_level: [0; 4],
            mid_level: [[0; 4]; 256],
            low_level: Box::new([const { None }; 65536]),
            bit_count: 0,
        }
    }

    /// Insert a bit at the given position
    /// 
    /// Returns true if the bit was not already set, false otherwise.
    /// 
    /// # Arguments
    /// * `position` - The position to set (0 to 16777215 = 256^3 - 1)
    /// 
    /// # Examples
    /// 
    /// ```
    /// # use solana_accounts_db::bit_array_tree::BitArrayTree;
    /// let mut tree = BitArrayTree::new();
    /// assert!(tree.insert(12345));
    /// assert!(!tree.insert(12345)); // Already set
    /// ```
    pub fn insert(&mut self, position: u32) -> bool {
        let (top_idx, mid_idx, low_idx, bit_pos) = Self::decode_position(position);
        
        // Ensure low-level array is allocated
        if self.low_level[low_idx as usize].is_none() {
            self.low_level[low_idx as usize] = Some(Box::new([0; 4]));
        }
        
        let low_array = self.low_level[low_idx as usize].as_mut().unwrap();
        let word_idx = (bit_pos / 64) as usize;
        let bit_idx = bit_pos % 64;
        let mask = 1u64 << bit_idx;
        
        // Check if bit is already set
        if low_array[word_idx] & mask != 0 {
            return false;
        }
        
        // Set the bit at all levels
        low_array[word_idx] |= mask;
        self.mid_level[mid_idx as usize][word_idx] |= mask;
        self.top_level[word_idx] |= mask;
        
        self.bit_count += 1;
        true
    }

    /// Remove a bit at the given position
    /// 
    /// Returns true if the bit was set, false otherwise.
    pub fn remove(&mut self, position: u32) -> bool {
        let (top_idx, mid_idx, low_idx, bit_pos) = Self::decode_position(position);
        
        // Check if low-level array exists
        let low_array = match self.low_level[low_idx as usize].as_mut() {
            Some(array) => array,
            None => return false, // Bit can't be set if array doesn't exist
        };
        
        let word_idx = (bit_pos / 64) as usize;
        let bit_idx = bit_pos % 64;
        let mask = 1u64 << bit_idx;
        
        // Check if bit is set
        if low_array[word_idx] & mask == 0 {
            return false;
        }
        
        // Clear the bit
        low_array[word_idx] &= !mask;
        self.bit_count -= 1;
        
        // Update mid-level if this was the last bit in the low-level array
        if low_array.iter().all(|&word| word == 0) {
            self.low_level[low_idx as usize] = None;
            let mid_word_idx = ((low_idx % 256) / 64) as usize;
            let mid_bit_idx = (low_idx % 256) % 64;
            let mid_mask = 1u64 << mid_bit_idx;
            self.mid_level[mid_idx as usize][mid_word_idx] &= !mid_mask;
            
            // Update top-level if this was the last bit in the mid-level array
            if self.mid_level[mid_idx as usize].iter().all(|&word| word == 0) {
                let top_word_idx = (mid_idx / 64) as usize;
                let top_bit_idx = mid_idx % 64;
                let top_mask = 1u64 << top_bit_idx;
                self.top_level[top_word_idx] &= !top_mask;
            }
        }
        
        true
    }

    /// Check if a bit is set at the given position
    pub fn contains(&self, position: u32) -> bool {
        let (_, _, low_idx, bit_pos) = Self::decode_position(position);
        
        match self.low_level[low_idx as usize].as_ref() {
            Some(low_array) => {
                let word_idx = (bit_pos / 64) as usize;
                let bit_idx = bit_pos % 64;
                (low_array[word_idx] & (1u64 << bit_idx)) != 0
            }
            None => false,
        }
    }

    /// Find the next set bit at or after the given position
    /// 
    /// Returns the position of the next set bit, or None if no set bits exist
    /// at or after the given position.
    /// 
    /// This is the core optimization function that enables efficient "jump-ahead"
    /// functionality by leveraging the hierarchical structure.
    pub fn find_next_set(&self, position: u32) -> Option<u32> {
        if position >= Self::MAX_POSITION {
            return None;
        }

        let (top_idx, mid_idx, low_idx, bit_pos) = Self::decode_position(position);
        
        // First, check if there's a set bit in the current low-level array at or after bit_pos
        if let Some(low_array) = self.low_level[low_idx as usize].as_ref() {
            if let Some(next_bit) = Self::find_next_set_in_array(low_array, bit_pos) {
                return Some(Self::encode_position(top_idx, mid_idx, low_idx, next_bit));
            }
        }
        
        // Check remaining low-level arrays in current mid-level
        for next_low_idx in (low_idx + 1)..((mid_idx + 1) * 256).min(65536) {
            if let Some(low_array) = self.low_level[next_low_idx as usize].as_ref() {
                if let Some(next_bit) = Self::find_next_set_in_array(low_array, 0) {
                    return Some(Self::encode_position(
                        top_idx, 
                        mid_idx, 
                        next_low_idx, 
                        next_bit
                    ));
                }
            }
        }
        
        // Check remaining mid-level arrays in current top-level
        for next_mid_idx in (mid_idx + 1)..((top_idx + 1) * 256).min(256) {
            if self.mid_level[next_mid_idx as usize].iter().any(|&word| word != 0) {
                // Find first set bit in this mid-level array
                for low_idx_offset in 0..256 {
                    let next_low_idx = next_mid_idx * 256 + low_idx_offset;
                    if next_low_idx >= 65536 {
                        break;
                    }
                    if let Some(low_array) = self.low_level[next_low_idx as usize].as_ref() {
                        if let Some(next_bit) = Self::find_next_set_in_array(low_array, 0) {
                            return Some(Self::encode_position(
                                top_idx,
                                next_mid_idx,
                                next_low_idx,
                                next_bit
                            ));
                        }
                    }
                }
            }
        }
        
        // Check remaining top-level arrays
        for next_top_idx in (top_idx + 1)..4 {
            if self.top_level[next_top_idx] != 0 {
                // Find first set bit in this top-level range
                for mid_idx_offset in 0..256 {
                    let next_mid_idx = next_top_idx * 256 + mid_idx_offset;
                    if next_mid_idx >= 256 {
                        break;
                    }
                    if self.mid_level[next_mid_idx as usize].iter().any(|&word| word != 0) {
                        for low_idx_offset in 0..256 {
                            let next_low_idx = next_mid_idx * 256 + low_idx_offset;
                            if next_low_idx >= 65536 {
                                break;
                            }
                            if let Some(low_array) = self.low_level[next_low_idx as usize].as_ref() {
                                if let Some(next_bit) = Self::find_next_set_in_array(low_array, 0) {
                                    return Some(Self::encode_position(
                                        next_top_idx,
                                        next_mid_idx,
                                        next_low_idx,
                                        next_bit
                                    ));
                                }
                            }
                        }
                    }
                }
            }
        }
        
        None
    }

    /// Find the previous set bit at or before the given position
    pub fn find_prev_set(&self, position: u32) -> Option<u32> {
        let (top_idx, mid_idx, low_idx, bit_pos) = Self::decode_position(position);
        
        // First, check if there's a set bit in the current low-level array at or before bit_pos
        if let Some(low_array) = self.low_level[low_idx as usize].as_ref() {
            if let Some(prev_bit) = Self::find_prev_set_in_array(low_array, bit_pos) {
                return Some(Self::encode_position(top_idx, mid_idx, low_idx, prev_bit));
            }
        }
        
        // Check previous low-level arrays in current mid-level
        if low_idx > mid_idx * 256 {
            for prev_low_idx in ((mid_idx * 256)..(low_idx)).rev() {
                if let Some(low_array) = self.low_level[prev_low_idx as usize].as_ref() {
                    if let Some(prev_bit) = Self::find_prev_set_in_array(low_array, 255) {
                        return Some(Self::encode_position(
                            top_idx,
                            mid_idx,
                            prev_low_idx,
                            prev_bit
                        ));
                    }
                }
            }
        }
        
        // Check previous mid-level arrays in current top-level
        if mid_idx > top_idx * 256 {
            for prev_mid_idx in ((top_idx * 256)..(mid_idx)).rev() {
                if self.mid_level[prev_mid_idx as usize].iter().any(|&word| word != 0) {
                    // Find last set bit in this mid-level array
                    for low_idx_offset in (0..256).rev() {
                        let prev_low_idx = prev_mid_idx * 256 + low_idx_offset;
                        if let Some(low_array) = self.low_level[prev_low_idx as usize].as_ref() {
                            if let Some(prev_bit) = Self::find_prev_set_in_array(low_array, 255) {
                                return Some(Self::encode_position(
                                    top_idx,
                                    prev_mid_idx,
                                    prev_low_idx,
                                    prev_bit
                                ));
                            }
                        }
                    }
                }
            }
        }
        
        // Check previous top-level arrays
        for prev_top_idx in (0..top_idx).rev() {
            if prev_top_idx < 4 && self.top_level[prev_top_idx] != 0 {
                // Find last set bit in this top-level range
                for mid_idx_offset in (0..256).rev() {
                    let prev_mid_idx = prev_top_idx * 256 + mid_idx_offset;
                    if self.mid_level[prev_mid_idx as usize].iter().any(|&word| word != 0) {
                        for low_idx_offset in (0..256).rev() {
                            let prev_low_idx = prev_mid_idx * 256 + low_idx_offset;
                            if let Some(low_array) = self.low_level[prev_low_idx as usize].as_ref() {
                                if let Some(prev_bit) = Self::find_prev_set_in_array(low_array, 255) {
                                    return Some(Self::encode_position(
                                        prev_top_idx,
                                        prev_mid_idx,
                                        prev_low_idx,
                                        prev_bit
                                    ));
                                }
                            }
                        }
                    }
                }
            }
        }
        
        None
    }

    /// Get the number of set bits
    pub fn len(&self) -> usize {
        self.bit_count
    }

    /// Check if the tree is empty
    pub fn is_empty(&self) -> bool {
        self.bit_count == 0
    }

    /// Clear all bits
    pub fn clear(&mut self) {
        self.top_level = [0; 4];
        self.mid_level = [[0; 4]; 256];
        self.low_level = Box::new([const { None }; 65536]);
        self.bit_count = 0;
    }

    /// Maximum supported position (256^3 - 1)
    pub const MAX_POSITION: u32 = 16_777_215;

    /// Decode a position into its component indices
    /// 
    /// Returns (top_idx, mid_idx, low_idx, bit_pos) where:
    /// - top_idx: 0-255 (which group of 256 mid-level arrays)
    /// - mid_idx: 0-255 (which mid-level array) 
    /// - low_idx: 0-65535 (which low-level array)
    /// - bit_pos: 0-255 (which bit in the low-level array)
    fn decode_position(position: u32) -> (u32, u32, u32, u32) {
        let top_idx = position / 65536;  // 256 * 256
        let mid_idx = position / 256;
        let low_idx = (position / 256) * 256 + (position % 256) / 256;
        let bit_pos = position % 256;
        
        (top_idx, mid_idx, low_idx, bit_pos)
    }

    /// Encode component indices back to a position
    fn encode_position(top_idx: u32, mid_idx: u32, low_idx: u32, bit_pos: u32) -> u32 {
        let _ = top_idx; // top_idx is implicit in mid_idx calculation
        mid_idx * 256 + bit_pos
    }

    /// Find the next set bit in a 256-bit array starting from the given position
    fn find_next_set_in_array(array: &[u64; 4], start_bit: u32) -> Option<u32> {
        let start_word = (start_bit / 64) as usize;
        let start_bit_in_word = start_bit % 64;

        // Check the starting word, but only bits at or after start_bit_in_word
        if start_word < 4 {
            let mask = !0u64 << start_bit_in_word;
            let masked_word = array[start_word] & mask;
            if masked_word != 0 {
                return Some(start_word as u32 * 64 + masked_word.trailing_zeros());
            }

            // Check remaining words
            for word_idx in (start_word + 1)..4 {
                if array[word_idx] != 0 {
                    return Some(word_idx as u32 * 64 + array[word_idx].trailing_zeros());
                }
            }
        }

        None
    }

    /// Find the previous set bit in a 256-bit array ending at the given position
    fn find_prev_set_in_array(array: &[u64; 4], end_bit: u32) -> Option<u32> {
        let end_word = (end_bit / 64) as usize;
        let end_bit_in_word = end_bit % 64;

        // Check the ending word, but only bits at or before end_bit_in_word
        if end_word < 4 {
            let mask = (1u64 << (end_bit_in_word + 1)) - 1;
            let masked_word = array[end_word] & mask;
            if masked_word != 0 {
                return Some(end_word as u32 * 64 + 63 - masked_word.leading_zeros());
            }

            // Check previous words
            for word_idx in (0..end_word).rev() {
                if array[word_idx] != 0 {
                    return Some(word_idx as u32 * 64 + 63 - array[word_idx].leading_zeros());
                }
            }
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_operations() {
        let mut tree = BitArrayTree::new();
        assert!(tree.is_empty());
        assert_eq!(tree.len(), 0);

        // Insert a bit
        assert!(tree.insert(100));
        assert!(!tree.is_empty());
        assert_eq!(tree.len(), 1);
        assert!(tree.contains(100));
        assert!(!tree.contains(101));

        // Insert the same bit again
        assert!(!tree.insert(100));
        assert_eq!(tree.len(), 1);

        // Insert another bit
        assert!(tree.insert(500));
        assert_eq!(tree.len(), 2);
        assert!(tree.contains(500));

        // Remove a bit
        assert!(tree.remove(100));
        assert_eq!(tree.len(), 1);
        assert!(!tree.contains(100));
        assert!(tree.contains(500));

        // Remove non-existent bit
        assert!(!tree.remove(100));
        assert_eq!(tree.len(), 1);
    }

    #[test]
    fn test_find_next_set() {
        let mut tree = BitArrayTree::new();
        tree.insert(10);
        tree.insert(25);
        tree.insert(1000);
        tree.insert(5000);

        assert_eq!(tree.find_next_set(0), Some(10));
        assert_eq!(tree.find_next_set(10), Some(10));
        assert_eq!(tree.find_next_set(11), Some(25));
        assert_eq!(tree.find_next_set(25), Some(25));
        assert_eq!(tree.find_next_set(26), Some(1000));
        assert_eq!(tree.find_next_set(1000), Some(1000));
        assert_eq!(tree.find_next_set(1001), Some(5000));
        assert_eq!(tree.find_next_set(5000), Some(5000));
        assert_eq!(tree.find_next_set(5001), None);
    }

    #[test]
    fn test_find_prev_set() {
        let mut tree = BitArrayTree::new();
        tree.insert(10);
        tree.insert(25);
        tree.insert(1000);
        tree.insert(5000);

        assert_eq!(tree.find_prev_set(5000), Some(5000));
        assert_eq!(tree.find_prev_set(4999), Some(1000));
        assert_eq!(tree.find_prev_set(1000), Some(1000));
        assert_eq!(tree.find_prev_set(999), Some(25));
        assert_eq!(tree.find_prev_set(25), Some(25));
        assert_eq!(tree.find_prev_set(24), Some(10));
        assert_eq!(tree.find_prev_set(10), Some(10));
        assert_eq!(tree.find_prev_set(9), None);
    }

    #[test]
    fn test_clear() {
        let mut tree = BitArrayTree::new();
        tree.insert(100);
        tree.insert(200);
        tree.insert(300);
        
        assert_eq!(tree.len(), 3);
        assert!(!tree.is_empty());

        tree.clear();
        
        assert_eq!(tree.len(), 0);
        assert!(tree.is_empty());
        assert!(!tree.contains(100));
        assert!(!tree.contains(200));
        assert!(!tree.contains(300));
    }

    #[test]
    fn test_edge_cases() {
        let mut tree = BitArrayTree::new();

        // Test position 0
        assert!(tree.insert(0));
        assert!(tree.contains(0));
        assert_eq!(tree.find_next_set(0), Some(0));
        assert_eq!(tree.find_prev_set(0), Some(0));

        // Test maximum position
        assert!(tree.insert(BitArrayTree::MAX_POSITION));
        assert!(tree.contains(BitArrayTree::MAX_POSITION));
        assert_eq!(tree.find_next_set(BitArrayTree::MAX_POSITION), Some(BitArrayTree::MAX_POSITION));
        assert_eq!(tree.find_prev_set(BitArrayTree::MAX_POSITION), Some(BitArrayTree::MAX_POSITION));
    }

    #[test]
    fn test_large_gaps() {
        let mut tree = BitArrayTree::new();
        
        // Insert bits with large gaps to test hierarchical search
        tree.insert(0);
        tree.insert(100_000);
        tree.insert(10_000_000);
        
        // Test jump-ahead functionality
        assert_eq!(tree.find_next_set(1), Some(100_000));
        assert_eq!(tree.find_next_set(100_001), Some(10_000_000));
        
        // Test backward search
        assert_eq!(tree.find_prev_set(9_999_999), Some(100_000));
        assert_eq!(tree.find_prev_set(99_999), Some(0));
    }

    #[test]
    fn test_position_encoding_decoding() {
        // Test various positions to ensure encoding/decoding works correctly
        let test_positions = [0, 1, 255, 256, 257, 65535, 65536, BitArrayTree::MAX_POSITION];
        
        for &pos in &test_positions {
            let (top_idx, mid_idx, low_idx, bit_pos) = BitArrayTree::decode_position(pos);
            let encoded = BitArrayTree::encode_position(top_idx, mid_idx, low_idx, bit_pos);
            assert_eq!(pos, encoded, "Position {} failed round-trip encoding", pos);
        }
    }
}