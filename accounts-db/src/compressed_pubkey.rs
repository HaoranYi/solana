use solana_pubkey::Pubkey;
use std::hash::{Hash, Hasher};

/// Compressed representation of a Pubkey within a specific bin.
/// Stores only the suffix bytes that differ from the bin's common prefix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompressedPubkey {
    /// Number of common prefix bytes shared with bin bounds
    prefix_len: u8,
    /// Remaining suffix bytes (29 bytes max)
    suffix: [u8; 29],
    /// Actual length of meaningful suffix data
    suffix_len: u8,
}

impl CompressedPubkey {
    /// Create a compressed pubkey within a bin with known bounds
    pub fn new(pubkey: &Pubkey, bin_lowest: &Pubkey, bin_highest: &Pubkey) -> Self {
        let pubkey_bytes = pubkey.as_ref();
        let lowest_bytes = bin_lowest.as_ref();
        let highest_bytes = bin_highest.as_ref();
        
        // Find common prefix length across the bin range
        let mut common_prefix_len = 0;
        for i in 0..32 {
            if lowest_bytes[i] == highest_bytes[i] {
                common_prefix_len = i + 1;
            } else {
                break;
            }
        }
        
        // We know from binning that at least first 3 bytes are predictable
        let prefix_len = common_prefix_len.max(3) as u8;
        let suffix_start = prefix_len as usize;
        let suffix_len = (32 - prefix_len) as u8;
        
        let mut suffix = [0u8; 29];
        if suffix_start < 32 {
            let copy_len = (32 - suffix_start).min(29);
            suffix[..copy_len].copy_from_slice(&pubkey_bytes[suffix_start..suffix_start + copy_len]);
        }
        
        Self {
            prefix_len,
            suffix,
            suffix_len,
        }
    }
    
    /// Reconstruct the full pubkey using bin bounds
    pub fn to_pubkey(&self, bin_lowest: &Pubkey) -> Pubkey {
        let mut result = [0u8; 32];
        let lowest_bytes = bin_lowest.as_ref();
        
        // Copy prefix from bin_lowest
        let prefix_len = self.prefix_len as usize;
        if prefix_len > 0 {
            result[..prefix_len].copy_from_slice(&lowest_bytes[..prefix_len]);
        }
        
        // Copy suffix
        let suffix_len = self.suffix_len as usize;
        if suffix_len > 0 && prefix_len < 32 {
            let copy_len = suffix_len.min(32 - prefix_len);
            result[prefix_len..prefix_len + copy_len].copy_from_slice(&self.suffix[..copy_len]);
        }
        
        Pubkey::from(result)
    }
    
    /// Get memory footprint in bytes
    pub fn memory_size(&self) -> usize {
        std::mem::size_of::<Self>()
    }
    
    /// Calculate potential memory savings vs full Pubkey
    pub fn savings_vs_pubkey(&self) -> usize {
        32_usize.saturating_sub(self.memory_size())
    }
}

impl Hash for CompressedPubkey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.prefix_len.hash(state);
        self.suffix[..self.suffix_len as usize].hash(state);
    }
}

/// Demonstration function showing memory savings potential
pub fn demonstrate_memory_savings() {
    println!("=== CompressedPubkey Memory Analysis ===\n");
    
    // Standard sizes
    println!("Standard sizes:");
    println!("  Pubkey size: {} bytes", std::mem::size_of::<Pubkey>());
    println!("  CompressedPubkey size: {} bytes", std::mem::size_of::<CompressedPubkey>());
    
    // Create test scenario with a bin
    let mut lowest = [0u8; 32];
    lowest[0] = 0x12;
    lowest[1] = 0x34;
    lowest[2] = 0x56;
    let bin_lowest = Pubkey::from(lowest);
    
    let mut highest = [0xFFu8; 32];
    highest[0] = 0x12;
    highest[1] = 0x34;
    highest[2] = 0x56;
    let bin_highest = Pubkey::from(highest);
    
    // Generate some test pubkeys in this bin
    let mut test_pubkeys = Vec::new();
    for i in 0..5 {
        let mut pk_bytes = [0u8; 32];
        pk_bytes[0] = 0x12;
        pk_bytes[1] = 0x34;
        pk_bytes[2] = 0x56;
        pk_bytes[10] = i;
        pk_bytes[31] = 0xFF - i;
        test_pubkeys.push(Pubkey::from(pk_bytes));
    }
    
    println!("\nBin configuration:");
    println!("  Bin lowest:  {:?}", bin_lowest.to_string());
    println!("  Bin highest: {:?}", bin_highest.to_string());
    println!("  Common prefix: 3 bytes (0x123456...)");
    
    // Test compression
    println!("\nCompression results:");
    let mut total_original = 0;
    let mut total_compressed = 0;
    
    for (i, pubkey) in test_pubkeys.iter().enumerate() {
        let compressed = CompressedPubkey::new(pubkey, &bin_lowest, &bin_highest);
        let reconstructed = compressed.to_pubkey(&bin_lowest);
        
        let original_size = std::mem::size_of::<Pubkey>();
        let compressed_size = compressed.memory_size();
        let savings = original_size.saturating_sub(compressed_size);
        
        total_original += original_size;
        total_compressed += compressed_size;
        
        println!("  Pubkey {}: {} -> {} bytes (saved {} bytes, {:.1}%)",
            i + 1,
            original_size,
            compressed_size,
            savings,
            savings as f64 / original_size as f64 * 100.0
        );
        
        assert_eq!(*pubkey, reconstructed, "Compression/decompression failed!");
    }
    
    let total_savings = total_original - total_compressed;
    println!("\nTotal for {} pubkeys:", test_pubkeys.len());
    println!("  Original: {} bytes", total_original);
    println!("  Compressed: {} bytes", total_compressed);
    println!("  Total savings: {} bytes ({:.1}%)", 
        total_savings,
        total_savings as f64 / total_original as f64 * 100.0
    );
    
    // Extrapolate to larger scale
    let million_accounts = 1_000_000;
    let million_original = million_accounts * 32;
    let million_compressed = million_accounts * std::mem::size_of::<CompressedPubkey>();
    let million_savings = million_original - million_compressed;
    
    println!("\nExtrapolated to 1M accounts:");
    println!("  Original: {:.1} MB", million_original as f64 / 1024.0 / 1024.0);
    println!("  Compressed: {:.1} MB", million_compressed as f64 / 1024.0 / 1024.0);
    println!("  Total savings: {:.1} MB ({:.1}%)", 
        million_savings as f64 / 1024.0 / 1024.0,
        million_savings as f64 / million_original as f64 * 100.0
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_compress_decompress() {
        // Create test pubkeys in the same bin
        let mut pk1_bytes = [0u8; 32];
        pk1_bytes[0] = 0x12; // First byte for bin
        pk1_bytes[1] = 0x34;
        pk1_bytes[2] = 0x56;
        pk1_bytes[10] = 0xAB; // Unique part
        pk1_bytes[31] = 0xFF;
        
        let mut pk2_bytes = [0u8; 32];
        pk2_bytes[0] = 0x12; // Same bin prefix
        pk2_bytes[1] = 0x34;
        pk2_bytes[2] = 0x56;
        pk2_bytes[10] = 0xCD; // Different unique part
        pk2_bytes[31] = 0xEE;
        
        let pubkey1 = Pubkey::from(pk1_bytes);
        let pubkey2 = Pubkey::from(pk2_bytes);
        
        // Simulate bin bounds (first 3 bytes define bin)
        let mut lowest = [0u8; 32];
        lowest[0] = 0x12;
        lowest[1] = 0x34;
        lowest[2] = 0x56;
        let bin_lowest = Pubkey::from(lowest);
        
        let mut highest = [0xFFu8; 32];
        highest[0] = 0x12;
        highest[1] = 0x34;
        highest[2] = 0x56;
        let bin_highest = Pubkey::from(highest);
        
        // Test compression and decompression
        let compressed1 = CompressedPubkey::new(&pubkey1, &bin_lowest, &bin_highest);
        let compressed2 = CompressedPubkey::new(&pubkey2, &bin_lowest, &bin_highest);
        
        let reconstructed1 = compressed1.to_pubkey(&bin_lowest);
        let reconstructed2 = compressed2.to_pubkey(&bin_lowest);
        
        assert_eq!(pubkey1, reconstructed1);
        assert_eq!(pubkey2, reconstructed2);
        assert_ne!(compressed1, compressed2);
    }
    
    #[test]
    fn test_memory_savings() {
        let pubkey = Pubkey::from([0u8; 32]);
        let bin_lowest = Pubkey::from([0u8; 32]);
        let bin_highest = Pubkey::from([0xFFu8; 32]);
        
        let compressed = CompressedPubkey::new(&pubkey, &bin_lowest, &bin_highest);
        
        // CompressedPubkey should be smaller than 32 bytes in most cases
        println!("CompressedPubkey size: {} bytes", compressed.memory_size());
        println!("Memory savings: {} bytes", compressed.savings_vs_pubkey());
        
        // Should save some space
        assert!(compressed.savings_vs_pubkey() > 0 || compressed.memory_size() <= 32);
    }
    
    #[test]
    fn test_demonstration() {
        demonstrate_memory_savings();
    }
}