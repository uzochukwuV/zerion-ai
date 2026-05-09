use solana_program::program_error::ProgramError;

#[repr(u32)]
pub enum EscrowError {
    InvalidInstruction = 1,
    InvalidPda = 2,
    InvalidTreasury = 3,
    AssetNotAllowed = 4,
    DeadlineInPast = 5,
    FundingClosed = 6,
    DeadlineNotReached = 7,
    BudgetExceeded = 8,
    InvalidMint = 9,
    InvalidTokenAccount = 10,
    MathOverflow = 11,
}

impl From<EscrowError> for ProgramError {
    fn from(value: EscrowError) -> Self {
        ProgramError::Custom(value as u32)
    }
}

