#![allow(clippy::integer_arithmetic)]
#![feature(test)]

extern crate test;

use {
    solana_runtime::{
        account_info::AccountInfo,
        accounts_db::AccountsDb,
        accounts_index::{AccountsIndexScanResult, UpsertReclaim},
    },
    solana_sdk::{account::AccountSharedData, pubkey},
    test::Bencher,
};

// test bench_scan ... bench:   1,403,954 ns/iter (+/- 43,566)
#[bench]
fn bench_scan(bencher: &mut Bencher) {
    let db = AccountsDb::new_single_for_tests();
    let empty_account = AccountSharedData::default();
    let count = 10_000;
    let pubkeys = (0..count).map(|_| pubkey::new_rand()).collect::<Vec<_>>();

    pubkeys.iter().for_each(|k| {
        for slot in 0..2 {
            // each upsert here (to a different slot) adds a refcount of 1 since entry is NOT cached
            db.accounts_index.upsert(
                slot,
                slot,
                k,
                &empty_account,
                &solana_runtime::accounts_index::AccountSecondaryIndexes::default(),
                AccountInfo::default(),
                &mut Vec::default(),
                UpsertReclaim::IgnoreReclaims,
            );
        }
    });

    bencher.iter(move || {
        db.accounts_index.scan(
            pubkeys.iter(),
            |_k, _slot_refs, _entry| AccountsIndexScanResult::OnlyKeepInMemoryIfDirty,
            None,
            false,
        );
    });
}
