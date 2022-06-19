#![feature(test)]

extern crate test;

use {
    solana_runtime::stakes::*,
    solana_sdk::{account::AccountSharedData, pubkey::Pubkey, rent::Rent},
    solana_stake_program::stake_state,
    solana_vote_program::vote_state::{self},
    test::Bencher,
};

fn create_stake_account(stake: u64, vote_pubkey: &Pubkey) -> (Pubkey, AccountSharedData) {
    let stake_pubkey = solana_sdk::pubkey::new_rand();
    (
        stake_pubkey,
        stake_state::create_account(
            &stake_pubkey,
            vote_pubkey,
            &vote_state::create_account(vote_pubkey, &solana_sdk::pubkey::new_rand(), 0, 1),
            &Rent::free(),
            stake,
        ),
    )
}
/**
running 2 tests (3000)
test bench_stake_cache_write       ... bench:   8,758,355 ns/iter (+/- 3,730,077)
test bench_stake_cache_write_batch ... bench:   6,020,470 ns/iter (+/- 977,997)
*/
#[bench]
fn bench_stake_cache_write(bencher: &mut Bencher) {
    let mut accounts = vec![];

    for _i in 0..600_000 {
        let vote_pubkey = solana_sdk::pubkey::new_rand();
        let (stake_pubkey, stake_account) = create_stake_account(10, &vote_pubkey);
        accounts.push((stake_pubkey, stake_account));
    }

    let accounts = accounts.iter().map(|x| (&x.0, &x.1)).collect::<Vec<_>>();

    bencher.iter(|| {
        test::black_box({
            let stakes_cache = StakesCache::new(Stakes {
                epoch: 10,
                ..Stakes::default()
            });
            for (key, account) in &accounts {
                stakes_cache.check_and_store(key, account);
            }
            0
        })
    });
}

#[bench]
fn bench_stake_cache_write_batch(bencher: &mut Bencher) {
    let mut accounts = vec![];

    for _i in 0..600_000 {
        let vote_pubkey = solana_sdk::pubkey::new_rand();
        let (stake_pubkey, stake_account) = create_stake_account(10, &vote_pubkey);
        accounts.push((stake_pubkey, stake_account));
    }

    let accounts = accounts.iter().map(|x| (&x.0, &x.1)).collect::<Vec<_>>();

    bencher.iter(|| {
        test::black_box({
            let stakes_cache = StakesCache::new(Stakes {
                epoch: 10,
                ..Stakes::default()
            });
            stakes_cache.check_and_store_batch(&accounts);
            0
        })
    });
}
