//! A type to hold data for the [`EpochRewards` sysvar][sv].
//!
//! [sv]: https://docs.solana.com/developing/runtime-facilities/sysvars#EpochRewards
//!
//! The sysvar ID is declared in [`sysvar::epoch_rewards`].
//!
//! [`sysvar::epoch_rewards`]: crate::sysvar::epoch_rewards
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Default, Clone, AbiExample)]
pub struct EpochRewards {
    // total rewards for the current epoch, in lamports
    total_rewards: u64,

    // distributed rewards for  the current epoch, in lamports
    distributed_rewards: u64,

    // distribution of all staking rewards for the current
    // epoch will be completed before this block height
    distribution_complete_block_height: u64,
}

impl EpochRewards {
    pub fn new(
        total_rewards: u64,
        distributed_rewards: u64,
        distribution_complete_block_height: u64,
    ) -> Self {
        Self {
            total_rewards,
            distributed_rewards,
            distribution_complete_block_height,
        }
    }

    pub fn distribute(&mut self, amount: u64) {
        assert!(self.distributed_rewards + amount <= self.total_rewards);

        self.total_rewards -= amount;
        self.distributed_rewards += amount;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_epoch_rewards_new() {
        let epoch_rewards = EpochRewards::new(100, 0, 64);

        assert_eq!(epoch_rewards.total_rewards, 100);
        assert_eq!(epoch_rewards.distributed_rewards, 0);
        assert_eq!(epoch_rewards.distribution_complete_block_height, 64);
    }

    #[test]
    fn test_epoch_rewards_clone() {
        let epoch_rewards = EpochRewards::new(100, 0, 64);
        let epoch_rewards_cloned = epoch_rewards.clone();

        assert_eq!(epoch_rewards, epoch_rewards_cloned);
    }

    #[test]
    fn test_epoch_rewards_distribute() {
        let mut epoch_rewards = EpochRewards::new(100, 0, 64);
        epoch_rewards.distribute(100);

        assert_eq!(epoch_rewards.total_rewards, 0);
        assert_eq!(epoch_rewards.distributed_rewards, 100);
    }

    #[test]
    #[should_panic(
        expected = "assertion failed: self.distributed_rewards + amount <= self.total_rewards"
    )]
    fn test_epoch_rewards_distribute_panic() {
        let mut epoch_rewards = EpochRewards::new(100, 0, 64);
        epoch_rewards.distribute(200);
    }
}
