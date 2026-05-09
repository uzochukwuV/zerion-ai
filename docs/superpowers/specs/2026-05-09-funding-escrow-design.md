# Funding Escrow (Solana Devnet) — Design

## Summary

A simple funding escrow program where a creator posts a project with a budget and a deadline. Funders deposit into a vault. The creator cannot withdraw until the deadline. After the deadline, the creator withdraws the funds minus a 1% protocol fee.

## Goals (v1)

- Support funding in exactly one asset per project:
  - Native SOL, or
  - A whitelisted SPL mint (e.g., USDC on devnet).
- Enforce a time lock: creator cannot withdraw before `deadline_unix_ts`.
- Enforce a budget cap: do not accept funding that would exceed `budget_amount`.
- Take a 1% protocol fee to a fixed treasury address.
- Keep on-chain state minimal (no on-chain contributor list).

## Non-Goals (v1)

- Milestones, staged releases, or refunds.
- “All-or-nothing” funding (creator can withdraw after deadline even if not fully funded).
- Yield generation (may be added later via additional instructions).

## Cluster / Environment

- Target cluster: devnet.

## Fixed Protocol Parameters

- Protocol treasury (fee destination):
  - `37x9AGp1ipgNfGbuoEVxQtjT5RJnJss6pT3V49TDnm5p`
- Protocol fee:
  - 1% of the vault balance at withdrawal time.
  - Fee calculation: `fee = amount / 100` (integer division, rounds down).

## Asset Model

Each project has exactly one asset:

- SOL project:
  - Vault is a PDA-owned system account holding lamports.
- SPL project:
  - Asset is a mint pubkey.
  - Mint must be in a whitelist.
  - Vault is the mint’s token account owned by a vault-authority PDA.

### Whitelist (v1)

Whitelist is implemented as a program constant list of allowed SPL mint pubkeys.

- The list must include the intended devnet USDC mint.
- The list can be expanded in future versions or moved into a Config PDA (admin-managed) without changing the per-project interface.

## On-chain Accounts

### 1) Project State PDA (program-owned)

Seed suggestion:

- `["project", creator_pubkey, project_id_or_nonce]`

Fields:

- `creator: Pubkey`
- `asset_kind: u8` (0 = SOL, 1 = SPL)
- `mint: Pubkey` (only meaningful when SPL; otherwise default Pubkey)
- `budget_amount: u64` (lamports for SOL; base units for SPL)
- `deadline_unix_ts: i64`
- `total_funded: u64`
- `vault_bump: u8`
- `state_bump: u8`

### 2) Vault PDA (holds funds)

- SOL project:
  - A PDA-derived system account with 0 data that holds lamports.
- SPL project:
  - A token account (ATA) for `(vault_authority_pda, mint)` that holds tokens.
  - `vault_authority_pda` is a PDA controlled by the program.

## Instructions

### 1) InitializeProject

Inputs:

- `asset_kind`
- `mint` (required if SPL)
- `budget_amount`
- `deadline_unix_ts`

Validations:

- `budget_amount > 0`
- `deadline_unix_ts > Clock.unix_timestamp`
- If SPL: `mint` must be in whitelist

Effects:

- Create and initialize Project State PDA.
- Create the vault (SOL PDA account or SPL vault ATA).

### 2) Fund

Inputs:

- `amount`

Validations:

- `Clock.unix_timestamp < deadline_unix_ts`
- `total_funded + amount <= budget_amount`
- `amount > 0`
- If SPL:
  - Source token account mint matches project mint.
  - Source token account authority signs.

Effects:

- Transfer `amount` from funder to vault (SOL or SPL).
- Increment `total_funded` by `amount`.

### 3) Withdraw

Inputs:

- none (withdraws full vault balance)

Validations:

- `Clock.unix_timestamp >= deadline_unix_ts`
- `creator` must sign

Fee rule:

- Charge 1% protocol fee at withdrawal time:
  - `fee = vault_balance / 100`
  - `payout = vault_balance - fee`

Effects:

- Transfer `fee` to protocol treasury (SOL) or to the treasury’s token account (SPL).
- Transfer `payout` to creator (SOL) or to creator’s token account (SPL).
- Vault ends empty.

## Indexing / Displaying Funders

v1 does not store contributor lists on-chain. The frontend/backends can index funding transactions by:

- Watching transfers into the vault address for SOL, and/or
- Watching token transfers into the SPL vault token account for SPL.

## Security / Invariants

- Only the program can move vault funds (vault is controlled by PDA).
- Only the recorded `creator` can withdraw.
- Withdraw must fail before deadline.
- Funding must fail after deadline.
- Funding must fail if it would exceed budget.
- Fee is always taken exactly once per withdraw call.

## Future Extensions (Compatibility Notes)

- Yield generation:
  - Add `EnableYield/DisableYield` instructions to move assets from vault into a strategy account.
  - Keep withdrawal gated by deadline and still apply fee.
- Admin-managed whitelist:
  - Add Config PDA with an admin key to add/remove mints without redeploy.
