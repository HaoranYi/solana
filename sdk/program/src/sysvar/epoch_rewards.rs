//! Epoch rewards for current epoch
//!
//! The _epoch rewards_ sysvar provide access to the [`EpochRewards`] type.
//!

pub use crate::epoch_rewards::EpochRewards;
use crate::{impl_sysvar_get, program_error::ProgramError, sysvar::Sysvar};

crate::declare_sysvar_id!("SysvarEpochReward51111111111111111111111111", EpochRewards);

impl Sysvar for EpochRewards {
    impl_sysvar_get!(sol_get_epoch_rewards_sysvar);
}
