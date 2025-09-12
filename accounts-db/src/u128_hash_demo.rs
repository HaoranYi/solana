use solana_pubkey::Pubkey;
use std::collections::HashMap;

/// Demonstration of u128 hash-based storage approach for pubkeys
pub struct U128HashDemo {
    // Standard approach: full pubkey as key
    standard_map: HashMap<Pubkey, String>,
    
    // Hash-based approach: u128 hash with collision handling
    hash_map: HashMap<u128, Vec<(Pubkey, String)>>,
}

impl U128HashDemo {
    pub fn new() -> Self {
        Self {
            standard_map: HashMap::new(),
            hash_map: HashMap::new(),
        }
    }
    
    /// Convert pubkey to u128 hash (first 16 bytes)
    fn pubkey_to_hash(pubkey: &Pubkey) -> u128 {
        let bytes = pubkey.as_ref();
        u128::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
            bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15]
        ])
    }
    
    /// Insert into standard map
    pub fn insert_standard(&mut self, pubkey: Pubkey, value: String) {
        self.standard_map.insert(pubkey, value);
    }
    
    /// Insert into hash-based map
    pub fn insert_hash(&mut self, pubkey: Pubkey, value: String) {
        let hash = Self::pubkey_to_hash(&pubkey);
        self.hash_map.entry(hash).or_insert_with(Vec::new).push((pubkey, value));
    }
    
    /// Get from standard map
    pub fn get_standard(&self, pubkey: &Pubkey) -> Option<&String> {
        self.standard_map.get(pubkey)
    }
    
    /// Get from hash-based map
    pub fn get_hash(&self, pubkey: &Pubkey) -> Option<&String> {
        let hash = Self::pubkey_to_hash(pubkey);
        self.hash_map.get(&hash)?.iter()
            .find(|(pk, _)| pk == pubkey)
            .map(|(_, value)| value)
    }
    
    /// Calculate memory usage statistics
    pub fn memory_analysis(&self) -> MemoryAnalysis {
        let standard_key_size = std::mem::size_of::<Pubkey>(); // 32 bytes
        let hash_key_size = std::mem::size_of::<u128>(); // 16 bytes
        let value_size = std::mem::size_of::<String>(); // Assuming String overhead
        
        // Standard map memory usage
        let standard_entries = self.standard_map.len();
        let standard_memory = standard_entries * (standard_key_size + value_size);
        
        // Hash map memory usage (approximation)
        let hash_entries = self.hash_map.len();
        let total_collision_entries: usize = self.hash_map.values().map(|v| v.len()).sum();
        let hash_memory = hash_entries * (hash_key_size + std::mem::size_of::<Vec<(Pubkey, String)>>())
            + total_collision_entries * (std::mem::size_of::<Pubkey>() + value_size);
        
        // Collision statistics
        let max_collisions = self.hash_map.values().map(|v| v.len()).max().unwrap_or(0);
        let avg_collisions = if hash_entries > 0 {
            total_collision_entries as f64 / hash_entries as f64
        } else {
            0.0
        };
        let collision_rate = if total_collision_entries > 0 {
            (total_collision_entries - hash_entries) as f64 / total_collision_entries as f64
        } else {
            0.0
        };
        
        MemoryAnalysis {
            standard_memory,
            hash_memory,
            memory_savings: standard_memory.saturating_sub(hash_memory),
            savings_percentage: if standard_memory > 0 {
                (standard_memory.saturating_sub(hash_memory) as f64 / standard_memory as f64) * 100.0
            } else {
                0.0
            },
            total_entries: standard_entries,
            hash_buckets: hash_entries,
            max_collisions,
            avg_collisions,
            collision_rate,
        }
    }
}

#[derive(Debug)]
pub struct MemoryAnalysis {
    pub standard_memory: usize,
    pub hash_memory: usize,
    pub memory_savings: usize,
    pub savings_percentage: f64,
    pub total_entries: usize,
    pub hash_buckets: usize,
    pub max_collisions: usize,
    pub avg_collisions: f64,
    pub collision_rate: f64,
}

/// Demonstrate the u128 hash approach with sample data
pub fn demonstrate_u128_hash_approach() {
    println!("=== U128 Hash-based Pubkey Storage Analysis ===\n");
    
    let mut demo = U128HashDemo::new();
    
    // Generate test pubkeys with some diversity
    let test_data = vec![
        (Pubkey::from([1u8; 32]), "Account A".to_string()),
        (Pubkey::from([2u8; 32]), "Account B".to_string()),
        (Pubkey::from([3u8; 32]), "Account C".to_string()),
    ];
    
    // Add more varied test cases
    for i in 0..100 {
        let mut bytes = [0u8; 32];
        bytes[0] = (i / 256) as u8;
        bytes[1] = (i % 256) as u8;
        bytes[15] = ((i * 7) % 256) as u8; // Add some variation in the hash part
        bytes[31] = ((i * 13) % 256) as u8;
        let pubkey = Pubkey::from(bytes);
        let value = format!("Account {}", i);
        
        demo.insert_standard(pubkey.clone(), value.clone());
        demo.insert_hash(pubkey, value);
    }
    
    // Add the initial test data
    for (pubkey, value) in test_data {
        demo.insert_standard(pubkey.clone(), value.clone());
        demo.insert_hash(pubkey, value);
    }
    
    // Verify correctness
    println!("Correctness verification:");
    let test_key = Pubkey::from([42u8; 32]);
    demo.insert_standard(test_key.clone(), "Test".to_string());
    demo.insert_hash(test_key.clone(), "Test".to_string());
    
    let standard_result = demo.get_standard(&test_key);
    let hash_result = demo.get_hash(&test_key);
    println!("  Standard result: {:?}", standard_result);
    println!("  Hash result: {:?}", hash_result);
    println!("  Results match: {}\n", standard_result == hash_result);
    
    // Memory analysis
    let analysis = demo.memory_analysis();
    println!("Memory Analysis:");
    println!("  Total entries: {}", analysis.total_entries);
    println!("  Standard map memory: {} bytes", analysis.standard_memory);
    println!("  Hash map memory: {} bytes", analysis.hash_memory);
    println!("  Memory savings: {} bytes ({:.1}%)", 
             analysis.memory_savings, analysis.savings_percentage);
    
    println!("\nCollision Analysis:");
    println!("  Hash buckets: {}", analysis.hash_buckets);
    println!("  Max collisions in bucket: {}", analysis.max_collisions);
    println!("  Average collisions per bucket: {:.2}", analysis.avg_collisions);
    println!("  Overall collision rate: {:.1}%", analysis.collision_rate * 100.0);
    
    // Theoretical analysis
    let key_size_savings_per_entry = 32 - 16; // 16 bytes saved per unique hash
    let theoretical_savings = analysis.hash_buckets * key_size_savings_per_entry;
    println!("\nTheoretical Analysis:");
    println!("  Key size savings per unique hash: {} bytes", key_size_savings_per_entry);
    println!("  Theoretical maximum savings: {} bytes", theoretical_savings);
    
    // Scale up analysis
    let million_scale = 1_000_000;
    let scaled_savings = (analysis.savings_percentage / 100.0) * (million_scale * 32) as f64;
    println!("\nScaled to 1M accounts:");
    println!("  Potential memory savings: {:.1} MB", scaled_savings / 1024.0 / 1024.0);
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_u128_hash_demo() {
        let mut demo = U128HashDemo::new();
        
        let pubkey1 = Pubkey::from([1u8; 32]);
        let pubkey2 = Pubkey::from([2u8; 32]);
        
        demo.insert_standard(pubkey1.clone(), "Value1".to_string());
        demo.insert_hash(pubkey1.clone(), "Value1".to_string());
        
        demo.insert_standard(pubkey2.clone(), "Value2".to_string());
        demo.insert_hash(pubkey2.clone(), "Value2".to_string());
        
        assert_eq!(demo.get_standard(&pubkey1), demo.get_hash(&pubkey1));
        assert_eq!(demo.get_standard(&pubkey2), demo.get_hash(&pubkey2));
        
        let analysis = demo.memory_analysis();
        assert_eq!(analysis.total_entries, 2);
        assert!(analysis.hash_buckets <= 2); // Could be less due to collisions
    }
    
    #[test]
    fn test_demonstration() {
        demonstrate_u128_hash_approach();
    }
}