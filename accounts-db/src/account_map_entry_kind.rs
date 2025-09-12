use std::collections::HashMap;
use solana_pubkey::Pubkey;

#[repr(C)]
union AccountMapEntryKindUnion<T> {
    pair: (u32, u32),
    boxed_vec: *mut Vec<T>,
}

pub struct AccountMapEntryKind<T> {
    data: AccountMapEntryKindUnion<T>,
}

impl<T> AccountMapEntryKind<T> {
    pub fn new_pair(a: u32, b: u32) -> Self {
        // For pairs, we set the least significant bit of the first u32 to 1 for tagging
        let tagged_a = a | 1;
        Self {
            data: AccountMapEntryKindUnion { pair: (tagged_a, b) },
        }
    }

    pub fn new_boxed_vec(vec: Vec<T>) -> Self {
        let boxed = Box::into_raw(Box::new(vec));
        // Box pointers are aligned, so last bit is always 0
        Self {
            data: AccountMapEntryKindUnion { boxed_vec: boxed },
        }
    }

    pub fn is_pair(&self) -> bool {
        unsafe {
            // Check the least significant bit of the first u32
            let first_u32 = self.data.pair.0;
            (first_u32 & 1) == 1
        }
    }

    pub fn as_pair(&self) -> Option<(u32, u32)> {
        if self.is_pair() {
            unsafe {
                // Remove the tag bit from the first u32
                let (tagged_a, b) = self.data.pair;
                let a = tagged_a & !1;  // Clear the tag bit
                Some((a, b))
            }
        } else {
            None
        }
    }

    pub fn as_boxed_vec(&self) -> Option<&Vec<T>> {
        if !self.is_pair() {
            unsafe { Some(&*self.data.boxed_vec) }
        } else {
            None
        }
    }

    pub fn as_boxed_vec_mut(&mut self) -> Option<&mut Vec<T>> {
        if !self.is_pair() {
            unsafe { Some(&mut *self.data.boxed_vec) }
        } else {
            None
        }
    }

    pub fn replace_with_pair(&mut self, a: u32, b: u32) {
        if !self.is_pair() {
            unsafe {
                let _ = Box::from_raw(self.data.boxed_vec);
            }
        }
        
        // Tag the first u32 by setting its least significant bit
        let tagged_a = a | 1;
        self.data = AccountMapEntryKindUnion { pair: (tagged_a, b) };
    }

    pub fn replace_with_vec(&mut self, vec: Vec<T>) {
        if !self.is_pair() {
            unsafe {
                let _ = Box::from_raw(self.data.boxed_vec);
            }
        }
        
        let boxed = Box::into_raw(Box::new(vec));
        self.data = AccountMapEntryKindUnion { boxed_vec: boxed };
    }
}

impl<T> Drop for AccountMapEntryKind<T> {
    fn drop(&mut self) {
        if !self.is_pair() {
            unsafe {
                let _ = Box::from_raw(self.data.boxed_vec);
            }
        }
    }
}

pub struct AccountMap<T> {
    map: HashMap<Pubkey, AccountMapEntryKind<T>>,
}

impl<T> AccountMap<T> {
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
        }
    }

    pub fn insert(&mut self, pubkey: Pubkey, entry: AccountMapEntryKind<T>) {
        self.map.insert(pubkey, entry);
    }

    pub fn get(&self, pubkey: &Pubkey) -> Option<&AccountMapEntryKind<T>> {
        self.map.get(pubkey)
    }

    pub fn get_mut(&mut self, pubkey: &Pubkey) -> Option<&mut AccountMapEntryKind<T>> {
        self.map.get_mut(pubkey)
    }

    pub fn process_lookup(&self, pubkey: &Pubkey) -> String {
        match self.get(pubkey) {
            Some(entry) => {
                if entry.is_pair() {
                    if let Some((a, b)) = entry.as_pair() {
                        format!("Found pair entry for pubkey {}: ({}, {})", pubkey, a, b)
                    } else {
                        "Error: entry marked as pair but failed to extract values".to_string()
                    }
                } else {
                    if let Some(vec) = entry.as_boxed_vec() {
                        format!("Found vector entry for pubkey {}: {} elements", pubkey, vec.len())
                    } else {
                        "Error: entry marked as vector but failed to extract values".to_string()
                    }
                }
            }
            None => format!("No entry found for pubkey {}", pubkey),
        }
    }

    pub fn update_entry<F>(&mut self, pubkey: &Pubkey, updater: F) -> bool 
    where 
        F: FnOnce(&mut AccountMapEntryKind<T>)
    {
        if let Some(entry) = self.get_mut(pubkey) {
            updater(entry);
            true
        } else {
            false
        }
    }

    pub fn update_pair(&mut self, pubkey: &Pubkey, new_a: u32, new_b: u32) -> bool {
        if let Some(entry) = self.get_mut(pubkey) {
            entry.replace_with_pair(new_a, new_b);
            true
        } else {
            false
        }
    }

    pub fn update_vec(&mut self, pubkey: &Pubkey, new_vec: Vec<T>) -> bool {
        if let Some(entry) = self.get_mut(pubkey) {
            entry.replace_with_vec(new_vec);
            true
        } else {
            false
        }
    }

    pub fn modify_vec<F>(&mut self, pubkey: &Pubkey, modifier: F) -> bool 
    where 
        F: FnOnce(&mut Vec<T>)
    {
        if let Some(entry) = self.get_mut(pubkey) {
            if let Some(vec) = entry.as_boxed_vec_mut() {
                modifier(vec);
                true
            } else {
                false
            }
        } else {
            false
        }
    }
}

pub fn demonstrate_lookup_processing<T: std::fmt::Debug>(account_map: &AccountMap<T>, pubkey: &Pubkey) {
    match account_map.get(pubkey) {
        Some(entry) => {
            println!("Processing lookup for pubkey: {}", pubkey);
            
            match (entry.is_pair(), entry.as_pair(), entry.as_boxed_vec()) {
                (true, Some((a, b)), _) => {
                    println!("  Type: Pair");
                    println!("  Values: {} and {}", a, b);
                    println!("  Sum: {}", a as u64 + b as u64);
                }
                (false, _, Some(vec)) => {
                    println!("  Type: Vector");
                    println!("  Length: {}", vec.len());
                    println!("  Contents: {:?}", vec);
                }
                _ => {
                    println!("  Error: Inconsistent entry state");
                }
            }
        }
        None => {
            println!("No entry found for pubkey: {}", pubkey);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem;

    #[test]
    fn test_mem_size() {
        println!("Size of AccountMapEntryKind<u64>: {} bytes", mem::size_of::<AccountMapEntryKind<u64>>());
        println!(
            "Size of AccountMapEntryKindUnion<u64>: {} bytes",
            mem::size_of::<AccountMapEntryKindUnion<u64>>()
        );
        println!("Size of (u32, u32): {} bytes", mem::size_of::<(u32, u32)>());
        println!(
            "Size of *mut Vec<u64>: {} bytes",
            mem::size_of::<*mut Vec<u64>>()
        );
        println!("Size of Vec<u64>: {} bytes", mem::size_of::<Vec<u64>>());

        let pair: AccountMapEntryKind<u64> = AccountMapEntryKind::new_pair(42, 123);
        let boxed_vec: AccountMapEntryKind<u64> = AccountMapEntryKind::new_boxed_vec(vec![1, 2, 3, 4, 5]);

        println!(
            "Memory used by pair instance: {} bytes",
            mem::size_of_val(&pair)
        );
        println!(
            "Memory used by boxed_vec instance: {} bytes",
            mem::size_of_val(&boxed_vec)
        );

        // Test functionality
        assert!(pair.is_pair());
        assert!(!boxed_vec.is_pair());

        if let Some((a, b)) = pair.as_pair() {
            println!("Pair values: ({}, {})", a, b);
            assert_eq!(a, 42);
            assert_eq!(b, 123);
        }

        if let Some(vec) = boxed_vec.as_boxed_vec() {
            println!("Vec values: {:?}", vec);
            assert_eq!(vec, &[1, 2, 3, 4, 5]);
        }
    }

    #[test]
    fn test_generic_types() {
        // Test with different types
        let pair_u32: AccountMapEntryKind<u32> = AccountMapEntryKind::new_pair(100, 200);
        let vec_strings: AccountMapEntryKind<String> = AccountMapEntryKind::new_boxed_vec(vec!["hello".to_string(), "world".to_string()]);
        let vec_i8: AccountMapEntryKind<i8> = AccountMapEntryKind::new_boxed_vec(vec![-1, 0, 1, 2]);

        println!("Size of AccountMapEntryKind<u32>: {} bytes", mem::size_of::<AccountMapEntryKind<u32>>());
        println!("Size of AccountMapEntryKind<String>: {} bytes", mem::size_of::<AccountMapEntryKind<String>>());
        println!("Size of AccountMapEntryKind<i8>: {} bytes", mem::size_of::<AccountMapEntryKind<i8>>());

        // All should be 8 bytes regardless of T
        assert_eq!(mem::size_of::<AccountMapEntryKind<u32>>(), 8);
        assert_eq!(mem::size_of::<AccountMapEntryKind<String>>(), 8);
        assert_eq!(mem::size_of::<AccountMapEntryKind<i8>>(), 8);

        // Test functionality
        assert!(pair_u32.is_pair());
        assert!(!vec_strings.is_pair());
        assert!(!vec_i8.is_pair());

        if let Some((a, b)) = pair_u32.as_pair() {
            assert_eq!(a, 100);
            assert_eq!(b, 200);
        }

        if let Some(strings) = vec_strings.as_boxed_vec() {
            assert_eq!(strings, &["hello".to_string(), "world".to_string()]);
        }

        if let Some(numbers) = vec_i8.as_boxed_vec() {
            assert_eq!(numbers, &[-1, 0, 1, 2]);
        }
    }

    #[test]
    fn test_pair_memory_layout() {
        let pair: AccountMapEntryKind<u64> = AccountMapEntryKind::new_pair(42, 123);

        // View raw memory layout
        let bytes = unsafe {
            std::slice::from_raw_parts(&pair as *const _ as *const u8, mem::size_of::<AccountMapEntryKind<u64>>())
        };

        println!("Raw bytes: {:02x?}", bytes);

        // Show the u32 values in memory
        unsafe {
            let (a, b) = pair.data.pair;
            println!("First u32 (tagged): 0x{:08x} = {}", a, a);
            println!("Second u32: 0x{:08x} = {}", b, b);
            println!("First u32 binary: {:032b}", a);
            println!("Last bit of first u32: {}", a & 1);
        }

        // Memory layout visualization
        println!("\nMemory layout (8 bytes total):");
        unsafe {
            let ptr_value = pair.data.boxed_vec as usize;
            println!("Tagged 8-byte value: 0x{:016x}", ptr_value);
            println!("Last bit (tag): {}", ptr_value & 1);
        }

        // Show original values can be recovered
        if let Some((orig_a, orig_b)) = pair.as_pair() {
            println!("Recovered values: ({}, {})", orig_a, orig_b);
        }
    }

    #[test]
    fn test_lookup_processing() {
        let mut account_map: AccountMap<u64> = AccountMap::new();
        
        let pubkey1 = Pubkey::new_unique();
        let pubkey2 = Pubkey::new_unique();
        let pubkey3 = Pubkey::new_unique();
        
        let pair_entry = AccountMapEntryKind::new_pair(100, 200);
        let vec_entry = AccountMapEntryKind::new_boxed_vec(vec![10, 20, 30, 40, 50]);
        
        account_map.insert(pubkey1, pair_entry);
        account_map.insert(pubkey2, vec_entry);
        
        let result1 = account_map.process_lookup(&pubkey1);
        let result2 = account_map.process_lookup(&pubkey2);
        let result3 = account_map.process_lookup(&pubkey3);
        
        assert!(result1.contains("Found pair entry"));
        assert!(result1.contains("(100, 200)"));
        
        assert!(result2.contains("Found vector entry"));
        assert!(result2.contains("5 elements"));
        
        assert!(result3.contains("No entry found"));
        
        println!("Lookup results:");
        println!("1. {}", result1);
        println!("2. {}", result2);
        println!("3. {}", result3);
        
        println!("\nDetailed processing:");
        demonstrate_lookup_processing(&account_map, &pubkey1);
        demonstrate_lookup_processing(&account_map, &pubkey2);
        demonstrate_lookup_processing(&account_map, &pubkey3);
    }

    #[test]
    fn test_match_patterns() {
        let mut account_map: AccountMap<String> = AccountMap::new();
        
        let pubkey1 = Pubkey::new_unique();
        let pubkey2 = Pubkey::new_unique();
        
        let entry1 = AccountMapEntryKind::new_pair(42, 123);
        let entry2 = AccountMapEntryKind::new_boxed_vec(vec!["hello".to_string(), "world".to_string()]);
        
        account_map.insert(pubkey1, entry1);
        account_map.insert(pubkey2, entry2);
        
        match account_map.get(&pubkey1) {
            Some(entry) if entry.is_pair() => {
                if let Some((a, b)) = entry.as_pair() {
                    assert_eq!(a, 42);
                    assert_eq!(b, 123);
                }
            }
            _ => panic!("Expected pair entry"),
        }
        
        match account_map.get(&pubkey2) {
            Some(entry) if !entry.is_pair() => {
                if let Some(vec) = entry.as_boxed_vec() {
                    assert_eq!(vec.len(), 2);
                    assert_eq!(vec[0], "hello");
                    assert_eq!(vec[1], "world");
                }
            }
            _ => panic!("Expected vector entry"),
        }
    }

    #[test]
    fn test_get_mut_and_updates() {
        let mut account_map: AccountMap<u64> = AccountMap::new();
        
        let pubkey1 = Pubkey::new_unique();
        let pubkey2 = Pubkey::new_unique();
        let pubkey3 = Pubkey::new_unique();
        
        let pair_entry = AccountMapEntryKind::new_pair(10, 20);
        let vec_entry = AccountMapEntryKind::new_boxed_vec(vec![1, 2, 3]);
        
        account_map.insert(pubkey1, pair_entry);
        account_map.insert(pubkey2, vec_entry);
        
        assert!(account_map.update_pair(&pubkey1, 100, 200));
        if let Some(entry) = account_map.get(&pubkey1) {
            if let Some((a, b)) = entry.as_pair() {
                assert_eq!(a, 100);
                assert_eq!(b, 200);
            }
        }
        
        assert!(account_map.update_vec(&pubkey2, vec![10, 20, 30, 40]));
        if let Some(entry) = account_map.get(&pubkey2) {
            if let Some(vec) = entry.as_boxed_vec() {
                assert_eq!(vec, &[10, 20, 30, 40]);
            }
        }
        
        assert!(!account_map.update_pair(&pubkey3, 999, 888));
        
        assert!(account_map.modify_vec(&pubkey2, |vec| {
            vec.push(50);
            vec[0] = 5;
        }));
        
        if let Some(entry) = account_map.get(&pubkey2) {
            if let Some(vec) = entry.as_boxed_vec() {
                assert_eq!(vec, &[5, 20, 30, 40, 50]);
            }
        }
    }

    #[test]
    fn test_update_entry_closure() {
        let mut account_map: AccountMap<String> = AccountMap::new();
        
        let pubkey1 = Pubkey::new_unique();
        let pubkey2 = Pubkey::new_unique();
        
        let entry1 = AccountMapEntryKind::new_pair(1, 2);
        let entry2 = AccountMapEntryKind::new_boxed_vec(vec!["hello".to_string()]);
        
        account_map.insert(pubkey1, entry1);
        account_map.insert(pubkey2, entry2);
        
        let update_result = account_map.update_entry(&pubkey1, |entry| {
            entry.replace_with_pair(42, 123);
        });
        assert!(update_result);
        
        if let Some(entry) = account_map.get(&pubkey1) {
            if let Some((a, b)) = entry.as_pair() {
                assert_eq!(a, 42);
                assert_eq!(b, 123);
            }
        }
        
        let update_result2 = account_map.update_entry(&pubkey2, |entry| {
            if let Some(vec) = entry.as_boxed_vec_mut() {
                vec.push("world".to_string());
            }
        });
        assert!(update_result2);
        
        if let Some(entry) = account_map.get(&pubkey2) {
            if let Some(vec) = entry.as_boxed_vec() {
                assert_eq!(vec.len(), 2);
                assert_eq!(vec[0], "hello");
                assert_eq!(vec[1], "world");
            }
        }
    }

    #[test]
    fn test_type_conversion_updates() {
        let mut account_map: AccountMap<i32> = AccountMap::new();
        
        let pubkey = Pubkey::new_unique();
        
        let pair_entry = AccountMapEntryKind::new_pair(4, 10);
        account_map.insert(pubkey, pair_entry);
        
        if let Some(entry) = account_map.get(&pubkey) {
            assert!(entry.is_pair());
            if let Some((a, b)) = entry.as_pair() {
                assert_eq!(a, 4);
                assert_eq!(b, 10);
            }
        }
        
        assert!(account_map.update_vec(&pubkey, vec![-1, 0, 1]));
        
        if let Some(entry) = account_map.get(&pubkey) {
            assert!(!entry.is_pair());
            if let Some(vec) = entry.as_boxed_vec() {
                assert_eq!(vec, &[-1, 0, 1]);
            }
        }
        
        assert!(account_map.update_pair(&pubkey, 998, 776));
        
        if let Some(entry) = account_map.get(&pubkey) {
            assert!(entry.is_pair());
            if let Some((a, b)) = entry.as_pair() {
                assert_eq!(a, 998);
                assert_eq!(b, 776);
            }
        }
    }

    #[test]
    fn test_account_map() {
        let mut account_map: AccountMap<u64> = AccountMap::new();
        
        let pubkey1 = Pubkey::new_unique();
        let pubkey2 = Pubkey::new_unique();
        
        let entry1 = AccountMapEntryKind::new_pair(42, 123);
        let entry2 = AccountMapEntryKind::new_boxed_vec(vec![1, 2, 3, 4, 5]);
        
        account_map.insert(pubkey1, entry1);
        account_map.insert(pubkey2, entry2);
        
        if let Some(retrieved_entry1) = account_map.get(&pubkey1) {
            assert!(retrieved_entry1.is_pair());
            if let Some((a, b)) = retrieved_entry1.as_pair() {
                assert_eq!(a, 42);
                assert_eq!(b, 123);
            }
        }
        
        if let Some(retrieved_entry2) = account_map.get(&pubkey2) {
            assert!(!retrieved_entry2.is_pair());
            if let Some(vec) = retrieved_entry2.as_boxed_vec() {
                assert_eq!(vec, &[1, 2, 3, 4, 5]);
            }
        }
        
        let non_existent_pubkey = Pubkey::new_unique();
        assert!(account_map.get(&non_existent_pubkey).is_none());
    }

    #[test]
    fn test_boxed_vec_memory_layout() {
        let boxed_vec: AccountMapEntryKind<u64> = AccountMapEntryKind::new_boxed_vec(vec![1, 2, 3, 4, 5]);

        // View raw memory layout
        let bytes = unsafe {
            std::slice::from_raw_parts(
                &boxed_vec as *const _ as *const u8,
                mem::size_of::<AccountMapEntryKind<u64>>(),
            )
        };

        println!("Raw bytes: {:02x?}", bytes);

        // Show the pointer value
        unsafe {
            let ptr = boxed_vec.data.boxed_vec;
            println!("Pointer value: 0x{:016x} = {}", ptr as usize, ptr as usize);
            println!("Pointer binary: {:064b}", ptr as usize);
            println!("Last bit of pointer: {}", (ptr as usize) & 1);

            // Show what the pointer points to
            println!("Vec at pointer:");
            println!("  ptr: 0x{:016x}", (*ptr).as_ptr() as usize);
            println!("  capacity: {}", (*ptr).capacity());
            println!("  len: {}", (*ptr).len());
            println!("  data: {:?}", *ptr);
        }

        // Memory layout visualization
        println!("\nMemory layout (8 bytes total):");
        println!("Bytes 0-7: Pointer to Vec<u64> (aligned, last bit = 0)");

        // Show original values can be recovered
        if let Some(vec) = boxed_vec.as_boxed_vec() {
            println!("Recovered vector: {:?}", vec);
        }
    }
}
