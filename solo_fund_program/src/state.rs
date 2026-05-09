use borsh::{BorshDeserialize, BorshSerialize};

#[derive(BorshSerialize, BorshDeserialize, Clone, Copy, PartialEq, Eq)]
pub struct ProjectState {
    pub creator: [u8; 32],
    pub asset_kind: u8,
    pub mint: [u8; 32],
    pub project_id: u64,
    pub budget_amount: u64,
    pub deadline_unix_ts: i64,
    pub total_funded: u64,
    pub vault_bump: u8,
    pub state_bump: u8,
}

impl ProjectState {
    pub const LEN: usize = 32 + 1 + 32 + 8 + 8 + 8 + 8 + 1 + 1;
}
