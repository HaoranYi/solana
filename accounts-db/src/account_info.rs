//! AccountInfo represents a reference to AccountSharedData in either an AppendVec or the write cache.
//! AccountInfo is not persisted anywhere between program runs.
//! AccountInfo is purely runtime state.
//! Note that AccountInfo is saved to disk buckets during runtime, but disk buckets are recreated at startup.
use {
    crate::{
        accounts_db::AccountsFileId,
        accounts_file::ALIGN_BOUNDARY_OFFSET,
        accounts_index::{DiskIndexValue, IndexValue, IsCached},
        is_zero_lamport::IsZeroLamport,
    },
    modular_bitfield::prelude::*,
};

/// offset within an append vec to account data
pub type Offset = usize;

/// specify where account data is located
#[derive(Debug, PartialEq, Eq)]
pub enum StorageLocation {
    AppendVec(AccountsFileId, Offset),
    Cached,
}

impl StorageLocation {
    pub fn is_offset_equal(&self, other: &StorageLocation) -> bool {
        match self {
            StorageLocation::Cached => {
                matches!(other, StorageLocation::Cached) // technically, 2 cached entries match in offset
            }
            StorageLocation::AppendVec(_, offset) => {
                match other {
                    StorageLocation::Cached => {
                        false // 1 cached, 1 not
                    }
                    StorageLocation::AppendVec(_, other_offset) => other_offset == offset,
                }
            }
        }
    }
    pub fn is_store_id_equal(&self, other: &StorageLocation) -> bool {
        match self {
            StorageLocation::Cached => {
                matches!(other, StorageLocation::Cached) // 2 cached entries are same store id
            }
            StorageLocation::AppendVec(store_id, _) => {
                match other {
                    StorageLocation::Cached => {
                        false // 1 cached, 1 not
                    }
                    StorageLocation::AppendVec(other_store_id, _) => other_store_id == store_id,
                }
            }
        }
    }
}

/// how large the offset we store in AccountInfo is
/// Note this is a smaller datatype than 'Offset'
/// AppendVecs store accounts aligned to u64, so offset is always a multiple of 8 (sizeof(u64))
pub type OffsetReduced = u32;

/// This is an illegal value for 'offset'.
/// Account size on disk would have to be pointing to the very last 8 byte value in the max sized append vec.
/// That would mean there was a maximum size of 8 bytes for the last entry in the append vec.
/// A pubkey alone is 32 bytes, so there is no way for a valid offset to be this high of a value.
/// Realistically, a max offset is (1<<31 - 156) bytes or so for an account with zero data length. Of course, this
/// depends on the layout on disk, compression, etc. But, 8 bytes per account will never be possible.
/// So, we use this last value as a sentinel to say that the account info refers to an entry in the write cache.
const CACHED_OFFSET: OffsetReduced = (1 << (OffsetReduced::BITS - 1)) - 1;

#[bitfield(bits = 32)]
#[repr(C)]
#[derive(Debug, Default, Copy, Clone, Eq, PartialEq)]
pub struct PackedOffsetAndFlags {
    /// this provides 2^31 bits, which when multiplied by 8 (sizeof(u64)) = 16G, which is the maximum size of an append vec
    offset_reduced: B31,
    /// use 1 bit to specify that the entry is zero lamport
    is_zero_lamport: bool,
}

#[derive(Default, Debug, PartialEq, Eq, Clone, Copy)]
pub struct AccountInfo {
    /// index identifying the append storage
    store_id: AccountsFileId,

    /// offset = 'packed_offset_and_flags.offset_reduced()' * ALIGN_BOUNDARY_OFFSET into the storage
    /// Note this is a smaller type than 'Offset'
    account_offset_and_flags: PackedOffsetAndFlags,
}

impl IsZeroLamport for AccountInfo {
    fn is_zero_lamport(&self) -> bool {
        self.account_offset_and_flags.is_zero_lamport()
    }
}

impl IsCached for AccountInfo {
    fn is_cached(&self) -> bool {
        self.account_offset_and_flags.offset_reduced() == CACHED_OFFSET
    }
}

impl IndexValue for AccountInfo {}

impl DiskIndexValue for AccountInfo {}

impl IsCached for StorageLocation {
    fn is_cached(&self) -> bool {
        matches!(self, StorageLocation::Cached)
    }
}

/// We have to have SOME value for store_id when we are cached
const CACHE_VIRTUAL_STORAGE_ID: AccountsFileId = AccountsFileId::MAX;

impl AccountInfo {
    pub fn new(storage_location: StorageLocation, is_zero_lamport: bool) -> Self {
        let mut packed_offset_and_flags = PackedOffsetAndFlags::default();
        let store_id = match storage_location {
            StorageLocation::AppendVec(store_id, offset) => {
                let reduced_offset = Self::get_reduced_offset(offset);
                assert_ne!(
                    CACHED_OFFSET, reduced_offset,
                    "illegal offset for non-cached item"
                );
                packed_offset_and_flags.set_offset_reduced(Self::get_reduced_offset(offset));
                assert_eq!(
                    Self::reduced_offset_to_offset(packed_offset_and_flags.offset_reduced()),
                    offset,
                    "illegal offset"
                );
                store_id
            }
            StorageLocation::Cached => {
                packed_offset_and_flags.set_offset_reduced(CACHED_OFFSET);
                CACHE_VIRTUAL_STORAGE_ID
            }
        };
        packed_offset_and_flags.set_is_zero_lamport(is_zero_lamport);
        Self {
            store_id,
            account_offset_and_flags: packed_offset_and_flags,
        }
    }

    pub fn get_reduced_offset(offset: usize) -> OffsetReduced {
        (offset / ALIGN_BOUNDARY_OFFSET) as OffsetReduced
    }

    pub fn store_id(&self) -> AccountsFileId {
        // if the account is in a cached store, the store_id is meaningless
        assert!(!self.is_cached());
        self.store_id
    }

    pub fn offset(&self) -> Offset {
        Self::reduced_offset_to_offset(self.account_offset_and_flags.offset_reduced())
    }

    pub fn reduced_offset_to_offset(reduced_offset: OffsetReduced) -> Offset {
        (reduced_offset as Offset) * ALIGN_BOUNDARY_OFFSET
    }

    pub fn storage_location(&self) -> StorageLocation {
        if self.is_cached() {
            StorageLocation::Cached
        } else {
            StorageLocation::AppendVec(self.store_id, self.offset())
        }
    }
}

#[cfg(test)]
mod test {
    use {super::*, crate::append_vec::MAXIMUM_APPEND_VEC_FILE_SIZE};

    #[test]
    fn test_limits() {
        for offset in [
            // MAXIMUM_APPEND_VEC_FILE_SIZE is too big. That would be an offset at the first invalid byte in the max file size.
            // MAXIMUM_APPEND_VEC_FILE_SIZE - 8 bytes would reference the very last 8 bytes in the file size. It makes no sense to reference that since element sizes are always more than 8.
            // MAXIMUM_APPEND_VEC_FILE_SIZE - 16 bytes would reference the second to last 8 bytes in the max file size. This is still likely meaningless, but it is 'valid' as far as the index
            // is concerned.
            (MAXIMUM_APPEND_VEC_FILE_SIZE - 2 * (ALIGN_BOUNDARY_OFFSET as u64)) as Offset,
            0,
            ALIGN_BOUNDARY_OFFSET,
            4 * ALIGN_BOUNDARY_OFFSET,
        ] {
            let info = AccountInfo::new(StorageLocation::AppendVec(0, offset), true);
            assert!(info.offset() == offset);
        }
    }

    #[test]
    #[should_panic(expected = "illegal offset")]
    fn test_illegal_offset() {
        let offset = (MAXIMUM_APPEND_VEC_FILE_SIZE - (ALIGN_BOUNDARY_OFFSET as u64)) as Offset;
        AccountInfo::new(StorageLocation::AppendVec(0, offset), true);
    }

    #[test]
    #[should_panic(expected = "illegal offset")]
    fn test_alignment() {
        let offset = 1; // not aligned
        AccountInfo::new(StorageLocation::AppendVec(0, offset), true);
    }

    #[test]
    fn test_slot_list_memory_size() {
        use {
            crate::accounts_index::SlotList,
            solana_clock::Slot,
            std::{mem, mem::ManuallyDrop, sync::RwLock},
        };

        // Test empty slot list
        let empty_slot_list: RwLock<SlotList<AccountInfo>> = RwLock::new(SlotList::new());
        println!(
            "RwLock<SlotList<AccountInfo>> (empty): {} bytes",
            mem::size_of_val(&empty_slot_list)
        );

        // Test with 1 element (inline in SmallVec)
        let mut one_element = SlotList::new();
        one_element.push((
            100,
            AccountInfo::new(StorageLocation::AppendVec(1, 0), false),
        ));
        let one_slot_list: RwLock<SlotList<AccountInfo>> = RwLock::new(one_element);
        println!(
            "RwLock<SlotList<AccountInfo>> (1 element): {} bytes",
            mem::size_of_val(&one_slot_list)
        );

        // Test with 2 elements (spilled to heap in SmallVec)
        let mut two_elements = SlotList::new();
        two_elements.push((
            100,
            AccountInfo::new(StorageLocation::AppendVec(1, 0), false),
        ));
        two_elements.push((
            200,
            AccountInfo::new(StorageLocation::AppendVec(2, 0), false),
        ));
        let two_slot_list: RwLock<SlotList<AccountInfo>> = RwLock::new(two_elements);
        println!(
            "RwLock<SlotList<AccountInfo>> (2 elements): {} bytes",
            mem::size_of_val(&two_slot_list)
        );

        // Print component sizes for reference
        println!("\nComponent sizes:");
        println!("  AccountInfo: {} bytes", mem::size_of::<AccountInfo>());
        println!("  (Slot, AccountInfo): {} bytes", mem::size_of::<(u64, AccountInfo)>());
        println!("  SlotList<AccountInfo>: {} bytes", mem::size_of::<SlotList<AccountInfo>>());
        println!(
            "  RwLock<SlotList<AccountInfo>>: {} bytes",
            mem::size_of::<RwLock<SlotList<AccountInfo>>>()
        );

        // Test proposed SlotListRepr union (inline singleton)
        #[repr(C)]
        union SlotListRepr {
            singleton: (Slot, AccountInfo),
            #[allow(clippy::box_collection)]
            list: ManuallyDrop<Box<Vec<(Slot, AccountInfo)>>>,
        }

        println!("\nProposed SlotListRepr union (inline singleton):");
        println!("  SlotListRepr: {} bytes", mem::size_of::<SlotListRepr>());
        println!(
            "  RwLock<SlotListRepr>: {} bytes",
            mem::size_of::<RwLock<SlotListRepr>>()
        );
        println!("  Box<Vec<(Slot, AccountInfo)>>: {} bytes", mem::size_of::<Box<Vec<(Slot, AccountInfo)>>>());
        println!("  ManuallyDrop<Box<Vec<(Slot, AccountInfo)>>>: {} bytes", mem::size_of::<ManuallyDrop<Box<Vec<(Slot, AccountInfo)>>>>());

        // Test alternative SlotListRepr union (boxed singleton)
        #[repr(C)]
        union SlotListReprBoxed {
            singleton: ManuallyDrop<Box<(Slot, AccountInfo)>>,
            #[allow(clippy::box_collection)]
            list: ManuallyDrop<Box<Vec<(Slot, AccountInfo)>>>,
        }

        println!("\nAlternative SlotListRepr union (boxed singleton):");
        println!("  SlotListReprBoxed: {} bytes", mem::size_of::<SlotListReprBoxed>());
        println!(
            "  RwLock<SlotListReprBoxed>: {} bytes",
            mem::size_of::<RwLock<SlotListReprBoxed>>()
        );
        println!("  Box<(Slot, AccountInfo)>: {} bytes", mem::size_of::<Box<(Slot, AccountInfo)>>());
        println!("  ManuallyDrop<Box<(Slot, AccountInfo)>>: {} bytes", mem::size_of::<ManuallyDrop<Box<(Slot, AccountInfo)>>>());

        // Test AccountMapEntry<AccountInfo> structure
        use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU8};

        // Recreate the structure to measure
        #[derive(Debug, Default)]
        #[allow(dead_code)]
        struct AccountMapEntryMeta {
            pub dirty: AtomicBool,
            pub age: AtomicU8,
        }

        #[derive(Debug)]
        #[allow(dead_code)]
        struct AccountMapEntry {
            ref_count: AtomicU32,
            slot_list: RwLock<SlotList<AccountInfo>>,
            pub meta: AccountMapEntryMeta,
        }

        println!("\nAccountMapEntry<AccountInfo> (current):");
        println!("  AccountMapEntry: {} bytes", mem::size_of::<AccountMapEntry>());
        println!("  AccountMapEntryMeta: {} bytes", mem::size_of::<AccountMapEntryMeta>());
        println!("  AtomicU32 (ref_count): {} bytes", mem::size_of::<AtomicU32>());
        println!("  RwLock<SlotList<AccountInfo>> (slot_list): {} bytes", mem::size_of::<RwLock<SlotList<AccountInfo>>>());
        println!("  AtomicBool (dirty): {} bytes", mem::size_of::<AtomicBool>());
        println!("  AtomicU8 (age): {} bytes", mem::size_of::<AtomicU8>());

        // Test proposed AccountMapEntry with SlotListRepr
        #[allow(dead_code)]
        struct AccountMapEntryProposed {
            ref_count: AtomicU32,
            slot_list: RwLock<SlotListRepr>,
            pub meta: AccountMapEntryMeta,
        }

        println!("\nAccountMapEntry with SlotListRepr (proposed inline):");
        println!("  AccountMapEntry (proposed): {} bytes", mem::size_of::<AccountMapEntryProposed>());
        println!("  Savings: {} bytes ({:.1}% reduction)",
            mem::size_of::<AccountMapEntry>() - mem::size_of::<AccountMapEntryProposed>(),
            100.0 * (mem::size_of::<AccountMapEntry>() - mem::size_of::<AccountMapEntryProposed>()) as f64 / mem::size_of::<AccountMapEntry>() as f64
        );

        // Test AccountMapEntry with boxed singleton
        #[allow(dead_code)]
        struct AccountMapEntryProposedBoxed {
            ref_count: AtomicU32,
            slot_list: RwLock<SlotListReprBoxed>,
            pub meta: AccountMapEntryMeta,
        }

        println!("\nAccountMapEntry with SlotListReprBoxed (proposed boxed):");
        println!("  AccountMapEntry (proposed boxed): {} bytes", mem::size_of::<AccountMapEntryProposedBoxed>());
        println!("  Savings vs current: {} bytes ({:.1}% reduction)",
            mem::size_of::<AccountMapEntry>() - mem::size_of::<AccountMapEntryProposedBoxed>(),
            100.0 * (mem::size_of::<AccountMapEntry>() - mem::size_of::<AccountMapEntryProposedBoxed>()) as f64 / mem::size_of::<AccountMapEntry>() as f64
        );
        println!("  Difference vs inline: {} bytes",
            (mem::size_of::<AccountMapEntryProposedBoxed>() as i32) - (mem::size_of::<AccountMapEntryProposed>() as i32)
        );

        // Test with AtomicU16 ref_count
        use std::sync::atomic::AtomicU16;

        #[allow(dead_code)]
        struct AccountMapEntryU16RefCount {
            ref_count: AtomicU16,
            slot_list: RwLock<SlotList<AccountInfo>>,
            pub meta: AccountMapEntryMeta,
        }

        #[allow(dead_code)]
        struct AccountMapEntryU16RefCountBoxed {
            ref_count: AtomicU16,
            slot_list: RwLock<SlotListReprBoxed>,
            pub meta: AccountMapEntryMeta,
        }

        println!("\nAccountMapEntry with AtomicU16 ref_count:");
        println!("  AtomicU16: {} bytes", mem::size_of::<AtomicU16>());
        println!("  Current (AtomicU32 + SlotList): {} bytes", mem::size_of::<AccountMapEntry>());
        println!("  With AtomicU16 + SlotList: {} bytes", mem::size_of::<AccountMapEntryU16RefCount>());
        println!("  With AtomicU16 + SlotListReprBoxed: {} bytes", mem::size_of::<AccountMapEntryU16RefCountBoxed>());
        println!("  Savings (U16 vs U32 with SlotList): {} bytes",
            (mem::size_of::<AccountMapEntry>() as i32) - (mem::size_of::<AccountMapEntryU16RefCount>() as i32)
        );
        println!("  Savings (U16 vs U32 with boxed): {} bytes",
            (mem::size_of::<AccountMapEntryProposedBoxed>() as i32) - (mem::size_of::<AccountMapEntryU16RefCountBoxed>() as i32)
        );

        // Test field reordering to reduce padding
        #[derive(Debug)]
        #[allow(dead_code)]
        struct AccountMapEntryReordered {
            slot_list: RwLock<SlotList<AccountInfo>>,
            ref_count: AtomicU32,
            pub meta: AccountMapEntryMeta,
        }

        #[allow(dead_code)]
        struct AccountMapEntryReorderedBoxed {
            slot_list: RwLock<SlotListReprBoxed>,
            ref_count: AtomicU32,
            pub meta: AccountMapEntryMeta,
        }

        #[allow(dead_code)]
        struct AccountMapEntryReorderedU16 {
            slot_list: RwLock<SlotList<AccountInfo>>,
            ref_count: AtomicU16,
            pub meta: AccountMapEntryMeta,
        }

        #[allow(dead_code)]
        struct AccountMapEntryReorderedU16Boxed {
            slot_list: RwLock<SlotListReprBoxed>,
            ref_count: AtomicU16,
            pub meta: AccountMapEntryMeta,
        }

        println!("\n=== Field Reordering Analysis ===");
        println!("Original order (ref_count, slot_list, meta):");
        println!("  With AtomicU32 + SlotList: {} bytes", mem::size_of::<AccountMapEntry>());
        println!("  With AtomicU32 + Boxed: {} bytes", mem::size_of::<AccountMapEntryProposedBoxed>());

        println!("\nReordered (slot_list, ref_count, meta):");
        println!("  With AtomicU32 + SlotList: {} bytes", mem::size_of::<AccountMapEntryReordered>());
        println!("  With AtomicU32 + Boxed: {} bytes", mem::size_of::<AccountMapEntryReorderedBoxed>());
        println!("  With AtomicU16 + SlotList: {} bytes", mem::size_of::<AccountMapEntryReorderedU16>());
        println!("  With AtomicU16 + Boxed: {} bytes", mem::size_of::<AccountMapEntryReorderedU16Boxed>());

        println!("\nSavings from reordering:");
        println!("  U32 + SlotList: {} bytes",
            (mem::size_of::<AccountMapEntry>() as i32) - (mem::size_of::<AccountMapEntryReordered>() as i32));
        println!("  U32 + Boxed: {} bytes",
            (mem::size_of::<AccountMapEntryProposedBoxed>() as i32) - (mem::size_of::<AccountMapEntryReorderedBoxed>() as i32));
        println!("  U16 + Boxed: {} bytes",
            (mem::size_of::<AccountMapEntryProposedBoxed>() as i32) - (mem::size_of::<AccountMapEntryReorderedU16Boxed>() as i32));

        println!("\nBest option vs current (48 bytes):");
        let best_size = mem::size_of::<AccountMapEntryReorderedU16Boxed>();
        println!("  Reordered U16 + Boxed: {} bytes", best_size);
        println!("  Total savings: {} bytes ({:.1}% reduction)",
            48 - best_size,
            100.0 * (48 - best_size) as f64 / 48.0
        );

        // Explain why reordering doesn't help
        println!("\n=== Why Reordering Doesn't Help ===");
        println!("Understanding struct padding and alignment:");

        println!("\nOriginal layout (ref_count, slot_list, meta):");
        println!("  Offset 0-3:   ref_count (AtomicU32) = 4 bytes");
        println!("  Offset 4-7:   [padding] = 4 bytes  (align slot_list to 8)");
        println!("  Offset 8-47:  slot_list (RwLock) = 40 bytes (8-byte aligned)");
        println!("  Offset 48-49: meta (2 bytes)");
        println!("  Offset 50-55: [padding] = 6 bytes  (struct aligned to 8)");
        println!("  Total: 56 bytes? NO! Compiler optimizes to 48 bytes");

        println!("\nReordered layout (slot_list, ref_count, meta):");
        println!("  Offset 0-39:  slot_list (RwLock) = 40 bytes (8-byte aligned)");
        println!("  Offset 40-43: ref_count (AtomicU32) = 4 bytes");
        println!("  Offset 44-45: meta (2 bytes)");
        println!("  Offset 46-47: [padding] = 2 bytes  (struct aligned to 8)");
        println!("  Total: 48 bytes - SAME SIZE!");

        println!("\nWith boxed SlotListRepr (24 bytes instead of 40):");
        println!("  Offset 0-23:  slot_list (RwLock<SlotListReprBoxed>) = 24 bytes");
        println!("  Offset 24-27: ref_count (AtomicU32) = 4 bytes");
        println!("  Offset 28-29: meta (2 bytes)");
        println!("  Offset 30-31: [padding] = 2 bytes  (struct aligned to 8)");
        println!("  Total: 32 bytes - 16 bytes saved!");

        println!("\nKey insight:");
        println!("  - Reordering can't eliminate padding when total is already optimal");
        println!("  - Only reducing field sizes (slot_list: 40→24) actually saves memory");
        println!("  - Struct is always padded to align to largest field (8 bytes for RwLock)");

        // Show alignment requirements
        println!("\nAlignment requirements:");
        println!("  RwLock<SlotList<AccountInfo>>: align {} bytes", mem::align_of::<RwLock<SlotList<AccountInfo>>>());
        println!("  RwLock<SlotListReprBoxed>: align {} bytes", mem::align_of::<RwLock<SlotListReprBoxed>>());
        println!("  AtomicU32: align {} bytes", mem::align_of::<AtomicU32>());
        println!("  AtomicU16: align {} bytes", mem::align_of::<AtomicU16>());
        println!("  AccountMapEntryMeta: align {} bytes", mem::align_of::<AccountMapEntryMeta>());
    }

    #[test]
    fn test_hashmap_entry_size() {
        use {
            crate::accounts_index::SlotList,
            solana_pubkey::Pubkey,
            std::{collections::HashMap, mem, sync::{Arc, RwLock}},
        };

        // Recreate AccountMapEntry for testing
        #[derive(Debug, Default)]
        #[allow(dead_code)]
        struct AccountMapEntryMeta {
            pub dirty: std::sync::atomic::AtomicBool,
            pub age: std::sync::atomic::AtomicU8,
        }

        #[derive(Debug)]
        #[allow(dead_code)]
        struct AccountMapEntry {
            ref_count: std::sync::atomic::AtomicU32,
            slot_list: RwLock<SlotList<AccountInfo>>,
            pub meta: AccountMapEntryMeta,
        }

        println!("\n=== HashMap Entry Memory Analysis ===");
        println!("Component sizes:");
        println!("  Pubkey: {} bytes", mem::size_of::<Pubkey>());
        println!("  Arc<AccountMapEntry>: {} bytes", mem::size_of::<Arc<AccountMapEntry>>());
        println!("  AccountMapEntry (on heap): {} bytes", mem::size_of::<AccountMapEntry>());

        println!("\nHashMap entry (Pubkey, Arc<AccountMapEntry>):");
        println!("  Tuple size: {} bytes", mem::size_of::<(Pubkey, Arc<AccountMapEntry>)>());

        // Create actual HashMap to measure overhead
        type MapType = HashMap<Pubkey, Arc<AccountMapEntry>, ahash::RandomState>;

        let empty_map: MapType = HashMap::with_hasher(ahash::RandomState::new());
        let mut map_with_one: MapType = HashMap::with_hasher(ahash::RandomState::new());

        // Create a dummy entry
        let pubkey = Pubkey::new_unique();
        let entry = AccountMapEntry {
            ref_count: std::sync::atomic::AtomicU32::new(1),
            slot_list: RwLock::new(SlotList::new()),
            meta: AccountMapEntryMeta::default(),
        };
        map_with_one.insert(pubkey, Arc::new(entry));

        println!("\nHashMap overhead analysis:");
        println!("  Empty HashMap size: {} bytes", mem::size_of_val(&empty_map));
        println!("  HashMap with 1 entry size: {} bytes", mem::size_of_val(&map_with_one));

        // Estimate per-entry overhead
        println!("\nPer-entry cost in HashMap:");
        println!("  Pubkey (key): 32 bytes");
        println!("  Arc<AccountMapEntry> (value): 8 bytes (pointer)");
        println!("  HashMap overhead per bucket: ~8 bytes (hash + metadata)");
        println!("  AccountMapEntry on heap: 48 bytes");
        println!("  Arc control block: ~16 bytes (strong/weak counts)");
        println!("  ----------------------------------------");
        println!("  Total per map entry: ~112 bytes");
        println!("    - In HashMap: 32 + 8 + 8 = 48 bytes");
        println!("    - On heap: 48 + 16 = 64 bytes");

        println!("\nWith proposed boxed SlotListRepr:");
        println!("  AccountMapEntry on heap: 32 bytes (instead of 48)");
        println!("  Total per entry: ~96 bytes (16 bytes saved)");
        println!("  Savings: 16 bytes per account (14.3% reduction)");

        // Test Box vs Arc
        println!("\n=== Box vs Arc Analysis ===");
        println!("Pointer sizes:");
        println!("  Arc<AccountMapEntry>: {} bytes", mem::size_of::<Arc<AccountMapEntry>>());
        println!("  Box<AccountMapEntry>: {} bytes", mem::size_of::<Box<AccountMapEntry>>());

        println!("\nHashMap entry sizes:");
        println!("  (Pubkey, Arc<AccountMapEntry>): {} bytes", mem::size_of::<(Pubkey, Arc<AccountMapEntry>)>());
        println!("  (Pubkey, Box<AccountMapEntry>): {} bytes", mem::size_of::<(Pubkey, Box<AccountMapEntry>)>());

        type BoxMapType = HashMap<Pubkey, Box<AccountMapEntry>, ahash::RandomState>;
        let empty_box_map: BoxMapType = HashMap::with_hasher(ahash::RandomState::new());
        println!("\nEmpty HashMap sizes:");
        println!("  HashMap<Pubkey, Arc<...>>: {} bytes", mem::size_of_val(&empty_map));
        println!("  HashMap<Pubkey, Box<...>>: {} bytes", mem::size_of_val(&empty_box_map));

        println!("\n=== Memory Comparison Per Entry ===");
        println!("\nCurrent (Arc + 48-byte AccountMapEntry):");
        println!("  In HashMap: 32 (Pubkey) + 8 (Arc ptr) + 8 (overhead) = 48 bytes");
        println!("  On heap: 16 (Arc control) + 48 (entry) = 64 bytes");
        println!("  Total: ~112 bytes");

        println!("\nWith Box (instead of Arc):");
        println!("  In HashMap: 32 (Pubkey) + 8 (Box ptr) + 8 (overhead) = 48 bytes");
        println!("  On heap: 0 (no control block) + 48 (entry) = 48 bytes");
        println!("  Total: ~96 bytes");
        println!("  Savings: 16 bytes (Arc control block eliminated!)");

        println!("\nWith Box + SlotListRepr (32-byte entry):");
        println!("  In HashMap: 32 (Pubkey) + 8 (Box ptr) + 8 (overhead) = 48 bytes");
        println!("  On heap: 0 (no control block) + 32 (entry) = 32 bytes");
        println!("  Total: ~80 bytes");
        println!("  Savings vs current: 32 bytes (28.6% reduction!)");

        println!("\n=== Trade-offs ===");
        println!("Arc advantages:");
        println!("  + Can share references across threads");
        println!("  + Reference counting for shared ownership");
        println!("\nBox advantages:");
        println!("  + No reference count overhead (16 bytes saved)");
        println!("  + Exclusive ownership (simpler semantics)");
        println!("  + No atomic operations for ref counting");
        println!("\n⚠️  IMPORTANT: Using Box requires checking if AccountMapEntry");
        println!("    is ever shared across threads or cloned via Arc::clone()");

        println!("\n=== Arc Sharing Analysis ===");
        println!("After investigation:");
        println!("  ✓ Arc IS needed - entries are cloned into evictions_age_possible");
        println!("  ✓ During eviction, same entry exists in HashMap AND evictions list");
        println!("  ✗ Cannot replace Arc with Box without architectural changes");
    }

    #[test]
    fn test_slotlist_vs_slotlistrepr_inline() {
        use {
            crate::accounts_index::SlotList,
            solana_clock::Slot,
            solana_pubkey::Pubkey,
            std::{mem, mem::ManuallyDrop, sync::{Arc, RwLock}},
        };

        // Define both union variants
        #[repr(C)]
        union SlotListRepr {
            singleton: (Slot, AccountInfo),
            #[allow(clippy::box_collection)]
            list: ManuallyDrop<Box<Vec<(Slot, AccountInfo)>>>,
        }

        #[repr(C)]
        union SlotListReprBoxed {
            singleton: ManuallyDrop<Box<(Slot, AccountInfo)>>,
            #[allow(clippy::box_collection)]
            list: ManuallyDrop<Box<Vec<(Slot, AccountInfo)>>>,
        }

        // AccountMapEntry variants
        #[derive(Debug, Default)]
        #[allow(dead_code)]
        struct AccountMapEntryMeta {
            pub dirty: std::sync::atomic::AtomicBool,
            pub age: std::sync::atomic::AtomicU8,
        }

        #[derive(Debug)]
        #[allow(dead_code)]
        struct AccountMapEntryCurrent {
            ref_count: std::sync::atomic::AtomicU32,
            slot_list: RwLock<SlotList<AccountInfo>>,
            pub meta: AccountMapEntryMeta,
        }

        #[allow(dead_code)]
        struct AccountMapEntryInline {
            ref_count: std::sync::atomic::AtomicU32,
            slot_list: RwLock<SlotListRepr>,
            pub meta: AccountMapEntryMeta,
        }

        #[allow(dead_code)]
        struct AccountMapEntryBoxed {
            ref_count: std::sync::atomic::AtomicU32,
            slot_list: RwLock<SlotListReprBoxed>,
            pub meta: AccountMapEntryMeta,
        }

        println!("\n=== Detailed Comparison: SlotList vs SlotListRepr ===");

        println!("\n1. Field sizes:");
        println!("  SlotList<AccountInfo>: {} bytes", mem::size_of::<SlotList<AccountInfo>>());
        println!("  SlotListRepr (inline singleton): {} bytes", mem::size_of::<SlotListRepr>());
        println!("  SlotListReprBoxed (boxed singleton): {} bytes", mem::size_of::<SlotListReprBoxed>());

        println!("\n2. RwLock-wrapped field sizes:");
        println!("  RwLock<SlotList<AccountInfo>>: {} bytes", mem::size_of::<RwLock<SlotList<AccountInfo>>>());
        println!("  RwLock<SlotListRepr>: {} bytes", mem::size_of::<RwLock<SlotListRepr>>());
        println!("  RwLock<SlotListReprBoxed>: {} bytes", mem::size_of::<RwLock<SlotListReprBoxed>>());

        println!("\n3. AccountMapEntry sizes:");
        println!("  Current (SlotList): {} bytes", mem::size_of::<AccountMapEntryCurrent>());
        println!("  With inline SlotListRepr: {} bytes", mem::size_of::<AccountMapEntryInline>());
        println!("  With boxed SlotListRepr: {} bytes", mem::size_of::<AccountMapEntryBoxed>());

        println!("\n4. Arc-wrapped sizes (as stored in HashMap):");
        println!("  Arc<Current>: {} bytes pointer + {} bytes on heap",
            mem::size_of::<Arc<AccountMapEntryCurrent>>(),
            mem::size_of::<AccountMapEntryCurrent>() + 16
        );
        println!("  Arc<Inline>: {} bytes pointer + {} bytes on heap",
            mem::size_of::<Arc<AccountMapEntryInline>>(),
            mem::size_of::<AccountMapEntryInline>() + 16
        );
        println!("  Arc<Boxed>: {} bytes pointer + {} bytes on heap",
            mem::size_of::<Arc<AccountMapEntryBoxed>>(),
            mem::size_of::<AccountMapEntryBoxed>() + 16
        );

        println!("\n5. HashMap entry tuple sizes:");
        println!("  (Pubkey, Arc<Current>): {} bytes", mem::size_of::<(Pubkey, Arc<AccountMapEntryCurrent>)>());
        println!("  (Pubkey, Arc<Inline>): {} bytes", mem::size_of::<(Pubkey, Arc<AccountMapEntryInline>)>());
        println!("  (Pubkey, Arc<Boxed>): {} bytes", mem::size_of::<(Pubkey, Arc<AccountMapEntryBoxed>)>());

        println!("\n=== Total Memory Per Entry (estimated) ===");
        let current_total = 48 + (mem::size_of::<AccountMapEntryCurrent>() + 16);
        let inline_total = 48 + (mem::size_of::<AccountMapEntryInline>() + 16);
        let boxed_total = 48 + (mem::size_of::<AccountMapEntryBoxed>() + 16);

        println!("  Current (SlotList): ~{} bytes", current_total);
        println!("    - In HashMap: 32 (Pubkey) + 8 (Arc) + 8 (overhead) = 48");
        println!("    - On heap: 16 (Arc control) + {} (entry) = {}",
            mem::size_of::<AccountMapEntryCurrent>(),
            mem::size_of::<AccountMapEntryCurrent>() + 16
        );

        println!("\n  Inline SlotListRepr: ~{} bytes", inline_total);
        println!("    - In HashMap: 48 bytes");
        println!("    - On heap: 16 (Arc control) + {} (entry) = {}",
            mem::size_of::<AccountMapEntryInline>(),
            mem::size_of::<AccountMapEntryInline>() + 16
        );
        println!("    - Savings: {} bytes ({:.1}%)",
            current_total - inline_total,
            100.0 * (current_total - inline_total) as f64 / current_total as f64
        );

        println!("\n  Boxed SlotListRepr: ~{} bytes", boxed_total);
        println!("    - In HashMap: 48 bytes");
        println!("    - On heap: 16 (Arc control) + {} (entry) = {}",
            mem::size_of::<AccountMapEntryBoxed>(),
            mem::size_of::<AccountMapEntryBoxed>() + 16
        );
        println!("    - Savings: {} bytes ({:.1}%)",
            current_total - boxed_total,
            100.0 * (current_total - boxed_total) as f64 / current_total as f64
        );

        println!("\n=== Recommendation ===");
        println!("  Best option: Boxed SlotListRepr");
        println!("  - Saves {} bytes per entry ({:.1}% reduction)",
            current_total - boxed_total,
            100.0 * (current_total - boxed_total) as f64 / current_total as f64
        );
        println!("  - At 100M accounts: {:.1} GB saved",
            (current_total - boxed_total) as f64 * 100_000_000.0 / 1_073_741_824.0
        );
        println!("\n  Why boxed is better than inline:");
        println!("  - Boxed: {} bytes entry vs Inline: {} bytes entry",
            mem::size_of::<AccountMapEntryBoxed>(),
            mem::size_of::<AccountMapEntryInline>()
        );
        println!("  - Trade-off: One heap allocation per singleton (most accounts)");
        println!("  - Benefit: {} bytes less memory per entry",
            mem::size_of::<AccountMapEntryInline>() - mem::size_of::<AccountMapEntryBoxed>()
        );
    }

    #[test]
    fn test_arc_vs_box_size() {
        use std::{mem, sync::Arc};

        #[derive(Debug, Default)]
        #[allow(dead_code)]
        struct AccountMapEntryMeta {
            pub dirty: std::sync::atomic::AtomicBool,
            pub age: std::sync::atomic::AtomicU8,
        }

        #[derive(Debug)]
        #[allow(dead_code)]
        struct AccountMapEntry48 {
            ref_count: std::sync::atomic::AtomicU32,
            slot_list: std::sync::RwLock<crate::accounts_index::SlotList<AccountInfo>>,
            pub meta: AccountMapEntryMeta,
        }

        println!("\n=== Arc vs Box: Pointer and Heap Size ===");

        println!("\n1. Pointer sizes (stack):");
        println!("  Arc<AccountMapEntry>: {} bytes", mem::size_of::<Arc<AccountMapEntry48>>());
        println!("  Box<AccountMapEntry>: {} bytes", mem::size_of::<Box<AccountMapEntry48>>());
        println!("  → Both are single pointers: SAME SIZE");

        println!("\n2. Heap allocation sizes:");
        println!("  AccountMapEntry size: {} bytes", mem::size_of::<AccountMapEntry48>());
        println!("  Arc overhead (control block): ~16 bytes");
        println!("    - strong count: 8 bytes (usize)");
        println!("    - weak count: 8 bytes (usize)");
        println!("  Box overhead: 0 bytes (no control block)");

        println!("\n3. Total heap size per allocation:");
        println!("  Arc<AccountMapEntry>: {} + 16 = 64 bytes", mem::size_of::<AccountMapEntry48>());
        println!("  Box<AccountMapEntry>: {} + 0 = 48 bytes", mem::size_of::<AccountMapEntry48>());
        println!("  → Arc uses 16 bytes more on heap");

        println!("\n4. Memory layout:");
        println!("  Arc<T>:");
        println!("    Stack: [8-byte pointer]");
        println!("    Heap:  [16-byte control block][T]");
        println!("           └─ strong/weak counts  └─ actual data");

        println!("\n  Box<T>:");
        println!("    Stack: [8-byte pointer]");
        println!("    Heap:  [T]");
        println!("           └─ actual data only");

        println!("\n5. Why Arc needs control block:");
        println!("  ✓ Reference counting (atomic strong count)");
        println!("  ✓ Weak reference support (atomic weak count)");
        println!("  ✓ Thread-safe shared ownership");
        println!("  ✓ Automatic cleanup when count reaches 0");

        println!("\n6. Why Box doesn't need control block:");
        println!("  ✓ Exclusive ownership (no sharing)");
        println!("  ✓ No reference counting needed");
        println!("  ✓ Simpler Drop implementation");

        println!("\n=== Summary ===");
        println!("  Pointer size: Arc == Box (8 bytes)");
        println!("  Heap overhead: Arc = 16 bytes, Box = 0 bytes");
        println!("  Total overhead: Arc costs 16 bytes more per allocation");
    }
}
