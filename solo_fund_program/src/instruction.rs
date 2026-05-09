use solana_program::{program_error::ProgramError, pubkey::Pubkey};

use crate::error::EscrowError;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Asset {
    Sol,
    Spl { mint: Pubkey },
}

pub enum EscrowInstruction {
    InitializeProject {
        asset: Asset,
        project_id: u64,
        budget_amount: u64,
        deadline_unix_ts: i64,
    },
    Fund { amount: u64 },
    Withdraw,
}

fn take<'a>(data: &'a [u8], offset: &mut usize, n: usize) -> Result<&'a [u8], ProgramError> {
    let end = offset
        .checked_add(n)
        .ok_or(EscrowError::InvalidInstruction)?;
    if end > data.len() {
        return Err(EscrowError::InvalidInstruction.into());
    }
    let out = &data[*offset..end];
    *offset = end;
    Ok(out)
}

fn read_u8(data: &[u8], offset: &mut usize) -> Result<u8, ProgramError> {
    Ok(take(data, offset, 1)?[0])
}

fn read_u64(data: &[u8], offset: &mut usize) -> Result<u64, ProgramError> {
    let bytes = take(data, offset, 8)?;
    Ok(u64::from_le_bytes(
        bytes
            .try_into()
            .map_err(|_| EscrowError::InvalidInstruction)?,
    ))
}

fn read_i64(data: &[u8], offset: &mut usize) -> Result<i64, ProgramError> {
    let bytes = take(data, offset, 8)?;
    Ok(i64::from_le_bytes(
        bytes
            .try_into()
            .map_err(|_| EscrowError::InvalidInstruction)?,
    ))
}

fn read_pubkey(data: &[u8], offset: &mut usize) -> Result<Pubkey, ProgramError> {
    let bytes = take(data, offset, 32)?;
    Ok(Pubkey::new_from_array(
        bytes
            .try_into()
            .map_err(|_| EscrowError::InvalidInstruction)?,
    ))
}

impl EscrowInstruction {
    pub fn unpack(input: &[u8]) -> Result<Self, ProgramError> {
        let mut offset = 0usize;
        let tag = read_u8(input, &mut offset)?;
        match tag {
            0 => {
                let asset_kind = read_u8(input, &mut offset)?;
                let mint = read_pubkey(input, &mut offset)?;
                let project_id = read_u64(input, &mut offset)?;
                let budget_amount = read_u64(input, &mut offset)?;
                let deadline_unix_ts = read_i64(input, &mut offset)?;
                let asset = match asset_kind {
                    0 => Asset::Sol,
                    1 => Asset::Spl { mint },
                    _ => return Err(EscrowError::InvalidInstruction.into()),
                };
                Ok(Self::InitializeProject {
                    asset,
                    project_id,
                    budget_amount,
                    deadline_unix_ts,
                })
            }
            1 => Ok(Self::Fund {
                amount: read_u64(input, &mut offset)?,
            }),
            2 => Ok(Self::Withdraw),
            _ => Err(EscrowError::InvalidInstruction.into()),
        }
    }
}
