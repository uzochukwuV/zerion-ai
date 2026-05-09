use std::str::FromStr;

use borsh::{BorshDeserialize, BorshSerialize};
use solana_program::{
    account_info::{next_account_info, AccountInfo},
    clock::Clock,
    entrypoint::ProgramResult,
    program::{invoke, invoke_signed},
    program_error::ProgramError,
    program_pack::Pack,
    pubkey::Pubkey,
    rent::Rent,
    sysvar::{Sysvar, SysvarSerialize},
};
use solana_system_interface::instruction as system_instruction;
use spl_associated_token_account::get_associated_token_address;
use spl_token::state::Account as TokenAccount;

use crate::{
    error::EscrowError,
    instruction::{Asset, EscrowInstruction},
    state::ProjectState,
};

const TREASURY_STR: &str = "37x9AGp1ipgNfGbuoEVxQtjT5RJnJss6pT3V49TDnm5p";
const WHITELIST_MINTS: [&str; 1] = ["4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZJDncDU"];

fn treasury_pubkey() -> Pubkey {
    Pubkey::from_str(TREASURY_STR).unwrap()
}

fn is_whitelisted_mint(mint: &Pubkey) -> bool {
    WHITELIST_MINTS
        .iter()
        .any(|s| Pubkey::from_str(s).ok().as_ref() == Some(mint))
}

fn derive_state_pda(program_id: &Pubkey, creator: &Pubkey, project_id: u64) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[b"project", creator.as_ref(), &project_id.to_le_bytes()],
        program_id,
    )
}

fn derive_vault_authority(program_id: &Pubkey, state_pda: &Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[b"vault", state_pda.as_ref()], program_id)
}

pub fn process(program_id: &Pubkey, accounts: &[AccountInfo], data: &[u8]) -> ProgramResult {
    let ix = EscrowInstruction::unpack(data)?;
    match ix {
        EscrowInstruction::InitializeProject {
            asset,
            project_id,
            budget_amount,
            deadline_unix_ts,
        } => process_initialize(program_id, accounts, asset, project_id, budget_amount, deadline_unix_ts),
        EscrowInstruction::Fund { amount } => process_fund(program_id, accounts, amount),
        EscrowInstruction::Withdraw => process_withdraw(program_id, accounts),
    }
}

fn process_initialize(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    asset: Asset,
    project_id: u64,
    budget_amount: u64,
    deadline_unix_ts: i64,
) -> ProgramResult {
    if budget_amount == 0 {
        return Err(EscrowError::InvalidInstruction.into());
    }

    match asset {
        Asset::Sol => {
            let mut it = accounts.iter();
            let creator = next_account_info(&mut it)?;
            if !creator.is_signer {
                return Err(ProgramError::MissingRequiredSignature);
            }
            let state_info = next_account_info(&mut it)?;
            let vault_info = next_account_info(&mut it)?;
            let system_program = next_account_info(&mut it)?;
            let rent_sysvar = next_account_info(&mut it)?;

            if !state_info.data_is_empty() {
                return Err(ProgramError::AccountAlreadyInitialized);
            }
            if !vault_info.data_is_empty() || **vault_info.lamports.borrow() != 0 {
                return Err(ProgramError::AccountAlreadyInitialized);
            }

            let clock = Clock::get()?;
            if deadline_unix_ts <= clock.unix_timestamp {
                return Err(EscrowError::DeadlineInPast.into());
            }

            let (expected_state, state_bump) = derive_state_pda(program_id, creator.key, project_id);
            if expected_state != *state_info.key {
                return Err(EscrowError::InvalidPda.into());
            }

            let (expected_vault, vault_bump) = derive_vault_authority(program_id, state_info.key);
            if expected_vault != *vault_info.key {
                return Err(EscrowError::InvalidPda.into());
            }

            let rent = Rent::from_account_info(rent_sysvar)?;
            let state_lamports = rent.minimum_balance(ProjectState::LEN);

            invoke_signed(
                &system_instruction::create_account(
                    creator.key,
                    state_info.key,
                    state_lamports,
                    ProjectState::LEN as u64,
                    program_id,
                ),
                &[creator.clone(), state_info.clone(), system_program.clone()],
                &[&[
                    b"project",
                    creator.key.as_ref(),
                    &project_id.to_le_bytes(),
                    &[state_bump],
                ]],
            )?;

            invoke_signed(
                &system_instruction::create_account(creator.key, vault_info.key, 1, 0, program_id),
                &[creator.clone(), vault_info.clone(), system_program.clone()],
                &[&[b"vault", state_info.key.as_ref(), &[vault_bump]]],
            )?;

            let state = ProjectState {
                creator: creator.key.to_bytes(),
                asset_kind: 0,
                mint: Pubkey::default().to_bytes(),
                project_id,
                budget_amount,
                deadline_unix_ts,
                total_funded: 0,
                vault_bump,
                state_bump,
            };
            state.serialize(&mut &mut state_info.data.borrow_mut()[..])?;
            Ok(())
        }
        Asset::Spl { mint } => {
            if !is_whitelisted_mint(&mint) {
                return Err(EscrowError::AssetNotAllowed.into());
            }

            let mut it = accounts.iter();
            let creator = next_account_info(&mut it)?;
            if !creator.is_signer {
                return Err(ProgramError::MissingRequiredSignature);
            }
            let state_info = next_account_info(&mut it)?;
            let vault_authority = next_account_info(&mut it)?;
            let vault_ata = next_account_info(&mut it)?;
            let mint_info = next_account_info(&mut it)?;
            let token_program = next_account_info(&mut it)?;
            let ata_program = next_account_info(&mut it)?;
            let system_program = next_account_info(&mut it)?;
            let rent_sysvar = next_account_info(&mut it)?;

            if *mint_info.key != mint {
                return Err(EscrowError::InvalidMint.into());
            }
            if *token_program.key != spl_token::id() {
                return Err(ProgramError::IncorrectProgramId);
            }
            if *ata_program.key != spl_associated_token_account::id() {
                return Err(ProgramError::IncorrectProgramId);
            }

            if !state_info.data_is_empty() {
                return Err(ProgramError::AccountAlreadyInitialized);
            }
            if !vault_ata.data_is_empty() {
                return Err(ProgramError::AccountAlreadyInitialized);
            }

            let clock = Clock::get()?;
            if deadline_unix_ts <= clock.unix_timestamp {
                return Err(EscrowError::DeadlineInPast.into());
            }

            let (expected_state, state_bump) = derive_state_pda(program_id, creator.key, project_id);
            if expected_state != *state_info.key {
                return Err(EscrowError::InvalidPda.into());
            }

            let (expected_vault_authority, vault_bump) =
                derive_vault_authority(program_id, state_info.key);
            if expected_vault_authority != *vault_authority.key {
                return Err(EscrowError::InvalidPda.into());
            }

            let expected_vault_ata = get_associated_token_address(vault_authority.key, &mint);
            if expected_vault_ata != *vault_ata.key {
                return Err(EscrowError::InvalidPda.into());
            }

            let rent = Rent::from_account_info(rent_sysvar)?;
            let state_lamports = rent.minimum_balance(ProjectState::LEN);

            invoke_signed(
                &system_instruction::create_account(
                    creator.key,
                    state_info.key,
                    state_lamports,
                    ProjectState::LEN as u64,
                    program_id,
                ),
                &[creator.clone(), state_info.clone(), system_program.clone()],
                &[&[
                    b"project",
                    creator.key.as_ref(),
                    &project_id.to_le_bytes(),
                    &[state_bump],
                ]],
            )?;

            let create_ata_ix = spl_associated_token_account::instruction::create_associated_token_account(
                creator.key,
                vault_authority.key,
                &mint,
                &spl_token::id(),
            );
            invoke(
                &create_ata_ix,
                &[
                    creator.clone(),
                    vault_ata.clone(),
                    vault_authority.clone(),
                    mint_info.clone(),
                    system_program.clone(),
                    token_program.clone(),
                    rent_sysvar.clone(),
                    ata_program.clone(),
                ],
            )?;

            let state = ProjectState {
                creator: creator.key.to_bytes(),
                asset_kind: 1,
                mint: mint.to_bytes(),
                project_id,
                budget_amount,
                deadline_unix_ts,
                total_funded: 0,
                vault_bump,
                state_bump,
            };
            state.serialize(&mut &mut state_info.data.borrow_mut()[..])?;
            Ok(())
        }
    }
}

fn process_fund(program_id: &Pubkey, accounts: &[AccountInfo], amount: u64) -> ProgramResult {
    if amount == 0 {
        return Err(EscrowError::InvalidInstruction.into());
    }

    let mut it = accounts.iter();
    let funder = next_account_info(&mut it)?;
    if !funder.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }

    let state_info = next_account_info(&mut it)?;
    let mut state = ProjectState::try_from_slice(&state_info.data.borrow())?;

    let new_total = state
        .total_funded
        .checked_add(amount)
        .ok_or(EscrowError::MathOverflow)?;
    if new_total > state.budget_amount {
        return Err(EscrowError::BudgetExceeded.into());
    }

    match state.asset_kind {
        0 => {
            let vault_info = next_account_info(&mut it)?;
            let system_program = next_account_info(&mut it)?;
            let clock_sysvar = next_account_info(&mut it)?;

            let creator = Pubkey::new_from_array(state.creator);
            let (expected_state, _) = derive_state_pda(program_id, &creator, state.project_id);
            if expected_state != *state_info.key {
                return Err(EscrowError::InvalidPda.into());
            }
            let (expected_vault, vault_bump) = derive_vault_authority(program_id, state_info.key);

            if expected_vault != *vault_info.key {
                return Err(EscrowError::InvalidPda.into());
            }
            if vault_bump != state.vault_bump {
                return Err(EscrowError::InvalidPda.into());
            }

            if *clock_sysvar.key != solana_program::sysvar::clock::id() {
                return Err(EscrowError::InvalidInstruction.into());
            }
            let clock = Clock::from_account_info(clock_sysvar)?;
            if clock.unix_timestamp >= state.deadline_unix_ts {
                return Err(EscrowError::FundingClosed.into());
            }

            invoke(
                &system_instruction::transfer(funder.key, vault_info.key, amount),
                &[funder.clone(), vault_info.clone(), system_program.clone()],
            )?;
        }
        1 => {
            let funder_token = next_account_info(&mut it)?;
            let vault_authority = next_account_info(&mut it)?;
            let vault_ata = next_account_info(&mut it)?;
            let token_program = next_account_info(&mut it)?;
            let clock_sysvar = next_account_info(&mut it)?;

            if *token_program.key != spl_token::id() {
                return Err(ProgramError::IncorrectProgramId);
            }

            let creator = Pubkey::new_from_array(state.creator);
            let mint = Pubkey::new_from_array(state.mint);
            let (expected_state, _) = derive_state_pda(program_id, &creator, state.project_id);
            if expected_state != *state_info.key {
                return Err(EscrowError::InvalidPda.into());
            }
            let (expected_vault_authority, vault_bump) =
                derive_vault_authority(program_id, state_info.key);
            let expected_vault_ata = get_associated_token_address(&expected_vault_authority, &mint);

            if *vault_authority.key != expected_vault_authority {
                return Err(EscrowError::InvalidPda.into());
            }
            if *vault_ata.key != expected_vault_ata {
                return Err(EscrowError::InvalidPda.into());
            }
            if vault_bump != state.vault_bump {
                return Err(EscrowError::InvalidPda.into());
            }

            if *clock_sysvar.key != solana_program::sysvar::clock::id() {
                return Err(EscrowError::InvalidInstruction.into());
            }
            let clock = Clock::from_account_info(clock_sysvar)?;
            if clock.unix_timestamp >= state.deadline_unix_ts {
                return Err(EscrowError::FundingClosed.into());
            }

            let ix = spl_token::instruction::transfer(
                &spl_token::id(),
                funder_token.key,
                vault_ata.key,
                funder.key,
                &[],
                amount,
            )?;
            invoke(
                &ix,
                &[
                    funder_token.clone(),
                    vault_ata.clone(),
                    funder.clone(),
                    token_program.clone(),
                ],
            )?;
        }
        _ => return Err(EscrowError::InvalidInstruction.into()),
    }

    state.total_funded = new_total;
    state.serialize(&mut &mut state_info.data.borrow_mut()[..])?;
    Ok(())
}

fn process_withdraw(program_id: &Pubkey, accounts: &[AccountInfo]) -> ProgramResult {
    let mut it = accounts.iter();
    let creator = next_account_info(&mut it)?;
    if !creator.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }
    let state_info = next_account_info(&mut it)?;
    let state = ProjectState::try_from_slice(&state_info.data.borrow())?;

    if creator.key.to_bytes() != state.creator {
        return Err(ProgramError::IllegalOwner);
    }

    let treasury = treasury_pubkey();

    match state.asset_kind {
        0 => {
            let vault_info = next_account_info(&mut it)?;
            let treasury_info = next_account_info(&mut it)?;
            let _system_program = next_account_info(&mut it)?;
            let clock_sysvar = next_account_info(&mut it)?;

            if *treasury_info.key != treasury {
                return Err(EscrowError::InvalidTreasury.into());
            }

            let (expected_state, _) = derive_state_pda(program_id, creator.key, state.project_id);
            if expected_state != *state_info.key {
                return Err(EscrowError::InvalidPda.into());
            }

            let (expected_vault, vault_bump) = derive_vault_authority(program_id, state_info.key);
            if expected_vault != *vault_info.key {
                return Err(EscrowError::InvalidPda.into());
            }
            if vault_bump != state.vault_bump {
                return Err(EscrowError::InvalidPda.into());
            }

            if *clock_sysvar.key != solana_program::sysvar::clock::id() {
                return Err(EscrowError::InvalidInstruction.into());
            }
            let clock = Clock::from_account_info(clock_sysvar)?;
            if clock.unix_timestamp < state.deadline_unix_ts {
                return Err(EscrowError::DeadlineNotReached.into());
            }

            let distributable = state.total_funded;
            let vault_balance = **vault_info.lamports.borrow();
            if vault_balance < distributable {
                return Err(EscrowError::MathOverflow.into());
            }

            let fee = distributable / 100;
            let payout = distributable
                .checked_sub(fee)
                .ok_or(EscrowError::MathOverflow)?;

            {
                let mut vault_lamports = vault_info.lamports.borrow_mut();
                **vault_lamports = (**vault_lamports)
                    .checked_sub(distributable)
                    .ok_or(EscrowError::MathOverflow)?;
            }

            if fee > 0 {
                let mut treasury_lamports = treasury_info.lamports.borrow_mut();
                **treasury_lamports = (**treasury_lamports)
                    .checked_add(fee)
                    .ok_or(EscrowError::MathOverflow)?;
            }
            if payout > 0 {
                let mut creator_lamports = creator.lamports.borrow_mut();
                **creator_lamports = (**creator_lamports)
                    .checked_add(payout)
                    .ok_or(EscrowError::MathOverflow)?;
            }

            Ok(())
        }
        1 => {
            let vault_authority = next_account_info(&mut it)?;
            let vault_ata = next_account_info(&mut it)?;
            let creator_ata = next_account_info(&mut it)?;
            let treasury_ata = next_account_info(&mut it)?;
            let token_program = next_account_info(&mut it)?;
            let clock_sysvar = next_account_info(&mut it)?;

            if *token_program.key != spl_token::id() {
                return Err(ProgramError::IncorrectProgramId);
            }

            if *clock_sysvar.key != solana_program::sysvar::clock::id() {
                return Err(EscrowError::InvalidInstruction.into());
            }
            let clock = Clock::from_account_info(clock_sysvar)?;
            if clock.unix_timestamp < state.deadline_unix_ts {
                return Err(EscrowError::DeadlineNotReached.into());
            }

            let mint = Pubkey::new_from_array(state.mint);
            let (expected_state, _) = derive_state_pda(program_id, creator.key, state.project_id);
            if expected_state != *state_info.key {
                return Err(EscrowError::InvalidPda.into());
            }
            let (expected_vault_authority, vault_bump) =
                derive_vault_authority(program_id, state_info.key);

            if expected_state != *state_info.key || expected_vault_authority != *vault_authority.key
            {
                return Err(EscrowError::InvalidPda.into());
            }
            if vault_bump != state.vault_bump {
                return Err(EscrowError::InvalidPda.into());
            }

            let expected_vault_ata = get_associated_token_address(&expected_vault_authority, &mint);
            if expected_vault_ata != *vault_ata.key {
                return Err(EscrowError::InvalidPda.into());
            }

            let expected_creator_ata = get_associated_token_address(creator.key, &mint);
            if expected_creator_ata != *creator_ata.key {
                return Err(EscrowError::InvalidTokenAccount.into());
            }

            let expected_treasury_ata = get_associated_token_address(&treasury, &mint);
            if expected_treasury_ata != *treasury_ata.key {
                return Err(EscrowError::InvalidTreasury.into());
            }

            let vault_token = TokenAccount::unpack(&vault_ata.data.borrow())
                .map_err(|_| EscrowError::InvalidTokenAccount)?;
            let amount = vault_token.amount;
            let fee = amount / 100;
            let payout = amount
                .checked_sub(fee)
                .ok_or(EscrowError::MathOverflow)?;

            if fee > 0 {
                let ix = spl_token::instruction::transfer(
                    &spl_token::id(),
                    vault_ata.key,
                    treasury_ata.key,
                    vault_authority.key,
                    &[],
                    fee,
                )?;
                invoke_signed(
                    &ix,
                    &[
                        vault_ata.clone(),
                        treasury_ata.clone(),
                        vault_authority.clone(),
                        token_program.clone(),
                    ],
                    &[&[b"vault", state_info.key.as_ref(), &[vault_bump]]],
                )?;
            }
            if payout > 0 {
                let ix = spl_token::instruction::transfer(
                    &spl_token::id(),
                    vault_ata.key,
                    creator_ata.key,
                    vault_authority.key,
                    &[],
                    payout,
                )?;
                invoke_signed(
                    &ix,
                    &[
                        vault_ata.clone(),
                        creator_ata.clone(),
                        vault_authority.clone(),
                        token_program.clone(),
                    ],
                    &[&[b"vault", state_info.key.as_ref(), &[vault_bump]]],
                )?;
            }

            Ok(())
        }
        _ => Err(EscrowError::InvalidInstruction.into()),
    }
}
