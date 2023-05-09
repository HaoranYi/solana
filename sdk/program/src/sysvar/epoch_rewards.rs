//! Epoch rewards for current epoch
//!
//! The _epoch rewards_ sysvar provide access to the [`EpochRewards`] type,
//! which tracks the progress of epoch rewards distribution. It includes the
//!   - total rewards for the current epoch, in lamports
//!   - distributed rewards for the current epoch, in lamports, so far
//!   - distribution completed block height, i.e. after this block height for distribution of all
//!     staking rewards for the current epoch will be completed should have been completed.
//!

pub use crate::epoch_rewards::EpochRewards;
use crate::{impl_sysvar_get, program_error::ProgramError, sysvar::Sysvar};

crate::declare_sysvar_id!("SysvarEpochReward51111111111111111111111111", EpochRewards);

impl Sysvar for EpochRewards {
    impl_sysvar_get!(sol_get_epoch_rewards_sysvar);
}
