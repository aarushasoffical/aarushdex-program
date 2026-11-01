use anchor_lang::prelude::*;
use anchor_spl::token::{self, Burn, Mint, MintTo, Token, TokenAccount, Transfer};

declare_id!("H6UKMGJB2ge93xvcE2WyoLDWYKtyqAKXKGid4nath757");

const MAX_FEE_BPS: u16 = 1_000; // 10%

// ===== Aarush Coin withdrawal fee =====
// Aarush Coin mint: 5eHSjLZahnEwSiWQizHp2ND69Wna3HjNJZEpQJMQhXLJ
// Treasury ATA:     9JeyfEq4QNK5y2xVrA9ue9wWVeHurowZkaLG9bQnVp3s
// Fee authority:    CdnDC6ZFRZaZ429RSqujV6effzcT8Lq3aRDHJuWiBaH4
// Mint decimals:    9
// Initial fee:      1 AARUSH = 1,000,000,000 base units

pub const AARUSH_MINT: Pubkey =
    pubkey!("5eHSjLZahnEwSiWQizHp2ND69Wna3HjNJZEpQJMQhXLJ");

pub const TREASURY_AARUSH_ATA: Pubkey =
    pubkey!("9JeyfEq4QNK5y2xVrA9ue9wWVeHurowZkaLG9bQnVp3s");

pub const AARUSH_WITHDRAWAL_FEE: u64 = 1_000_000_000;


#[program]
pub mod dex {
    use super::*;

    pub fn create_pool(ctx: Context<CreatePool>, fee_bps: u16) -> Result<()> {
        require!(fee_bps <= MAX_FEE_BPS, DexError::InvalidFee);

        // Canonical ordering prevents the same pair from getting two pools.
        require!(
            ctx.accounts.token_a_mint.key() < ctx.accounts.token_b_mint.key(),
            DexError::InvalidTokenOrder
        );

        let pool = &mut ctx.accounts.pool;
        pool.token_a_mint = ctx.accounts.token_a_mint.key();
        pool.token_b_mint = ctx.accounts.token_b_mint.key();
        pool.fee_bps = fee_bps;
        pool.reserve_a = 0;
        pool.reserve_b = 0;
        msg!("Pool created: {}", pool.key());
        Ok(())
    }

    pub fn add_liquidity(
        ctx: Context<AddLiquidity>,
        amount_a: u64,
        amount_b: u64,
        min_lp_out: u64,
    ) -> Result<()> {
        require!(amount_a > 0 && amount_b > 0, DexError::InvalidAmount);

        let reserve_a = ctx.accounts.pool.reserve_a;
        let reserve_b = ctx.accounts.pool.reserve_b;
        let total_lp = ctx.accounts.lp_mint.supply;

        let lp_out_u128 = if total_lp == 0 {
            let product = (amount_a as u128)
                .checked_mul(amount_b as u128)
                .ok_or(DexError::MathOverflow)?;
            integer_sqrt(product)
        } else {
            require!(reserve_a > 0 && reserve_b > 0, DexError::InvalidPoolState);

            let lp_a = (amount_a as u128)
                .checked_mul(total_lp as u128)
                .ok_or(DexError::MathOverflow)?
                / reserve_a as u128;

            let lp_b = (amount_b as u128)
                .checked_mul(total_lp as u128)
                .ok_or(DexError::MathOverflow)?
                / reserve_b as u128;

            lp_a.min(lp_b)
        };

        let lp_out = u64::try_from(lp_out_u128).map_err(|_| DexError::MathOverflow)?;

        require!(lp_out >= min_lp_out, DexError::SlippageExceeded);
        require!(lp_out > 0, DexError::InsufficientLiquidityMint);

        token::transfer(
            CpiContext::new(
                ctx.accounts.token_program.key(),
                Transfer {
                    from: ctx.accounts.user_a.to_account_info(),
                    to: ctx.accounts.vault_a.to_account_info(),
                    authority: ctx.accounts.user.to_account_info(),
                },
            ),
            amount_a,
        )?;

        token::transfer(
            CpiContext::new(
                ctx.accounts.token_program.key(),
                Transfer {
                    from: ctx.accounts.user_b.to_account_info(),
                    to: ctx.accounts.vault_b.to_account_info(),
                    authority: ctx.accounts.user.to_account_info(),
                },
            ),
            amount_b,
        )?;

        let pool_key = ctx.accounts.pool.key();
        let pool_seeds: &[&[u8]] = &[
            b"pool",
            ctx.accounts.pool.token_a_mint.as_ref(),
            ctx.accounts.pool.token_b_mint.as_ref(),
            &[ctx.bumps.pool],
        ];

        token::mint_to(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.key(),
                MintTo {
                    mint: ctx.accounts.lp_mint.to_account_info(),
                    to: ctx.accounts.user_lp.to_account_info(),
                    authority: ctx.accounts.pool.to_account_info(),
                },
                &[pool_seeds],
            ),
            lp_out,
        )?;
        let pool = &mut ctx.accounts.pool;

        pool.reserve_a = pool
            .reserve_a
            .checked_add(amount_a)
            .ok_or(DexError::MathOverflow)?;

        pool.reserve_b = pool
            .reserve_b
            .checked_add(amount_b)
            .ok_or(DexError::MathOverflow)?;

        msg!("Pool {} added liquidity; LP out: {}", pool_key, lp_out);
        Ok(())
    }
    pub fn remove_liquidity(
        ctx: Context<RemoveLiquidity>,
        lp_amount: u64,
        min_a_out: u64,
        min_b_out: u64,
    ) -> Result<()> {
        require!(lp_amount > 0, DexError::InvalidAmount);



        let reserve_a = ctx.accounts.pool.reserve_a;
        let reserve_b = ctx.accounts.pool.reserve_b;
        let total_lp = ctx.accounts.lp_mint.supply;

        require!(
            reserve_a > 0 && reserve_b > 0 && total_lp > 0,
            DexError::InvalidPoolState
        );

        let amount_a = u64::try_from(
            (lp_amount as u128)
                .checked_mul(reserve_a as u128)
                .ok_or(DexError::MathOverflow)?
                / total_lp as u128,
        )
        .map_err(|_| DexError::MathOverflow)?;

        let amount_b = u64::try_from(
            (lp_amount as u128)
                .checked_mul(reserve_b as u128)
                .ok_or(DexError::MathOverflow)?
                / total_lp as u128,
        )
        .map_err(|_| DexError::MathOverflow)?;

        require!(amount_a >= min_a_out, DexError::SlippageExceeded);
        require!(amount_b >= min_b_out, DexError::SlippageExceeded);
        require!(
            amount_a > 0 && amount_b > 0,
            DexError::InsufficientLiquidityBurn
        );

    // Collect 1 AARUSH withdrawal fee into the fixed treasury ATA.
    token::transfer(
        CpiContext::new(
            Token::id(),
            Transfer {
                from: ctx.accounts.user_aarush_ata.to_account_info(),
                to: ctx.accounts.treasury_aarush_ata.to_account_info(),
                authority: ctx.accounts.user.to_account_info(),
            },
        ),
        AARUSH_WITHDRAWAL_FEE,
    )?;

        token::burn(
            CpiContext::new(
                ctx.accounts.token_program.key(),
                Burn {
                    mint: ctx.accounts.lp_mint.to_account_info(),
                    from: ctx.accounts.user_lp.to_account_info(),
                    authority: ctx.accounts.user.to_account_info(),
                },
            ),
            lp_amount,
        )?;

        let pool_seeds: &[&[u8]] = &[
            b"pool",
            ctx.accounts.pool.token_a_mint.as_ref(),
            ctx.accounts.pool.token_b_mint.as_ref(),
            &[ctx.bumps.pool],
        ];

        token::transfer(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.key(),
                Transfer {
                    from: ctx.accounts.vault_a.to_account_info(),
                    to: ctx.accounts.user_a.to_account_info(),
                    authority: ctx.accounts.pool.to_account_info(),
                },
                &[pool_seeds],
            ),
            amount_a,
        )?;

        token::transfer(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.key(),
                Transfer {
                    from: ctx.accounts.vault_b.to_account_info(),
                    to: ctx.accounts.user_b.to_account_info(),
                    authority: ctx.accounts.pool.to_account_info(),
                },
                &[pool_seeds],
            ),
            amount_b,
        )?;

        // Keep recorded reserves synchronized with the withdrawal.
        ctx.accounts.pool.reserve_a = reserve_a
            .checked_sub(amount_a)
            .ok_or(DexError::MathOverflow)?;

        ctx.accounts.pool.reserve_b = reserve_b
            .checked_sub(amount_b)
            .ok_or(DexError::MathOverflow)?;

        Ok(())
    }
    #[derive(Accounts)]
    pub struct AddLiquidity<'info> {
        #[account(mut)]
        pub user: Signer<'info>,

        #[account(
        mut,
        seeds = [b"pool", pool.token_a_mint.as_ref(), pool.token_b_mint.as_ref()],
        bump
    )]
        pub pool: Box<Account<'info, Pool>>,

        #[account(
        mut,
        seeds = [b"vault_a", pool.key().as_ref()],
        bump,
        token::mint = token_a_mint,
        token::authority = pool
    )]
        pub vault_a: Box<Account<'info, TokenAccount>>,

        #[account(
        mut,
        seeds = [b"vault_b", pool.key().as_ref()],
        bump,
        token::mint = token_b_mint,
        token::authority = pool
    )]
        pub vault_b: Box<Account<'info, TokenAccount>>,

        #[account(
        mut,
        seeds = [b"lp_mint", pool.key().as_ref()],
        bump,
        mint::authority = pool
    )]
        pub lp_mint: Box<Account<'info, Mint>>,

        #[account(
        mut,
        constraint = user_a.owner == user.key() @ DexError::InvalidUserAccount,
        constraint = user_a.mint == token_a_mint.key() @ DexError::InvalidUserAccount
    )]
        pub user_a: Box<Account<'info, TokenAccount>>,

        #[account(
        mut,
        constraint = user_b.owner == user.key() @ DexError::InvalidUserAccount,
        constraint = user_b.mint == token_b_mint.key() @ DexError::InvalidUserAccount
    )]
        pub user_b: Box<Account<'info, TokenAccount>>,

        #[account(
        mut,
        constraint = user_lp.owner == user.key() @ DexError::InvalidUserAccount,
        constraint = user_lp.mint == lp_mint.key() @ DexError::InvalidLpAccount
    )]
        pub user_lp: Box<Account<'info, TokenAccount>>,

        #[account(address = pool.token_a_mint)]
        pub token_a_mint: Box<Account<'info, Mint>>,

        #[account(address = pool.token_b_mint)]
        pub token_b_mint: Box<Account<'info, Mint>>,

        pub token_program: Program<'info, Token>,
    }
    pub fn swap_a_for_b(
        ctx: Context<SwapAForB>,
        amount_in: u64,
        min_amount_out: u64,
    ) -> Result<()> {
        execute_swap(
            &mut ctx.accounts.pool,
            &ctx.accounts.token_program,
            &ctx.accounts.user,
            &ctx.accounts.user_a,
            &ctx.accounts.user_b,
            &ctx.accounts.vault_a,
            &ctx.accounts.vault_b,
            amount_in,
            min_amount_out,
            ctx.bumps.pool,
            true,
        )
    }

    pub fn swap_b_for_a(
        ctx: Context<SwapBForA>,
        amount_in: u64,
        min_amount_out: u64,
    ) -> Result<()> {
        execute_swap(
            &mut ctx.accounts.pool,
            &ctx.accounts.token_program,
            &ctx.accounts.user,
            &ctx.accounts.user_b,
            &ctx.accounts.user_a,
            &ctx.accounts.vault_a,
            &ctx.accounts.vault_b,
            amount_in,
            min_amount_out,
            ctx.bumps.pool,
            false,
        )
    }
}

fn execute_swap<'info>(
    pool: &mut Account<'info, Pool>,
    token_program: &Program<'info, Token>,
    user: &Signer<'info>,
    user_in: &Account<'info, TokenAccount>,
    user_out: &Account<'info, TokenAccount>,
    vault_a: &Account<'info, TokenAccount>,
    vault_b: &Account<'info, TokenAccount>,
    amount_in: u64,
    min_amount_out: u64,
    pool_bump: u8,
    a_for_b: bool,
) -> Result<()> {
    require!(amount_in > 0, DexError::InvalidAmount);

    // The pool reserve fields are the AMM's accounting state.
    // Normal AarushDEX operations keep these synchronized with the vaults.
    let reserve_in = if a_for_b {
        pool.reserve_a
    } else {
        pool.reserve_b
    };

    let reserve_out = if a_for_b {
        pool.reserve_b
    } else {
        pool.reserve_a
    };

    let amount_out = get_amount_out(amount_in, reserve_in, reserve_out, pool.fee_bps)?;

    require!(amount_out >= min_amount_out, DexError::SlippageExceeded);

    require!(
        amount_out > 0 && amount_out < reserve_out,
        DexError::InsufficientLiquidity
    );

    // User sends input tokens into the correct vault.
    token::transfer(
        CpiContext::new(
            token_program.key(),
            Transfer {
                from: user_in.to_account_info(),
                to: if a_for_b {
                    vault_a.to_account_info()
                } else {
                    vault_b.to_account_info()
                },
                authority: user.to_account_info(),
            },
        ),
        amount_in,
    )?;

    let pool_seeds: &[&[u8]] = &[
        b"pool",
        pool.token_a_mint.as_ref(),
        pool.token_b_mint.as_ref(),
        &[pool_bump],
    ];

    // Pool sends output tokens from the correct vault.
    token::transfer(
        CpiContext::new_with_signer(
            token_program.key(),
            Transfer {
                from: if a_for_b {
                    vault_b.to_account_info()
                } else {
                    vault_a.to_account_info()
                },
                to: user_out.to_account_info(),
                authority: pool.to_account_info(),
            },
            &[pool_seeds],
        ),
        amount_out,
    )?;

    // Keep the recorded reserves synchronized with the exact swap.
    if a_for_b {
        pool.reserve_a = reserve_in
            .checked_add(amount_in)
            .ok_or(DexError::MathOverflow)?;

        pool.reserve_b = reserve_out
            .checked_sub(amount_out)
            .ok_or(DexError::MathOverflow)?;
    } else {
        pool.reserve_b = reserve_in
            .checked_add(amount_in)
            .ok_or(DexError::MathOverflow)?;

        pool.reserve_a = reserve_out
            .checked_sub(amount_out)
            .ok_or(DexError::MathOverflow)?;
    }

    Ok(())
}

fn get_amount_out(amount_in: u64, reserve_in: u64, reserve_out: u64, fee_bps: u16) -> Result<u64> {
    require!(
        reserve_in > 0 && reserve_out > 0,
        DexError::InsufficientLiquidity
    );

    let fee_factor = 10_000u128
        .checked_sub(fee_bps as u128)
        .ok_or(DexError::InvalidFee)?;

    let amount_in_with_fee = (amount_in as u128)
        .checked_mul(fee_factor)
        .ok_or(DexError::MathOverflow)?;

    let numerator = amount_in_with_fee
        .checked_mul(reserve_out as u128)
        .ok_or(DexError::MathOverflow)?;

    let denominator = (reserve_in as u128)
        .checked_mul(10_000u128)
        .ok_or(DexError::MathOverflow)?
        .checked_add(amount_in_with_fee)
        .ok_or(DexError::MathOverflow)?;

    require!(denominator > 0, DexError::MathOverflow);

    u64::try_from(numerator / denominator).map_err(|_| DexError::MathOverflow.into())
}

fn integer_sqrt(value: u128) -> u128 {
    if value == 0 {
        return 0;
    }

    let mut x = value;
    let mut y = x / 2 + 1;

    while y < x {
        x = y;
        y = (x + value / x) / 2;
    }

    x
}
#[derive(Accounts)]
pub struct CreatePool<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,

    #[account(
        init,
        payer = payer,
        space = 8 + Pool::SIZE,
        seeds = [b"pool", token_a_mint.key().as_ref(), token_b_mint.key().as_ref()],
        bump
    )]
    pub pool: Account<'info, Pool>,

    pub token_a_mint: Account<'info, Mint>,
    pub token_b_mint: Account<'info, Mint>,

    #[account(
    init,
    payer = payer,
    seeds = [b"vault_a", pool.key().as_ref()],
    bump,
    token::mint = token_a_mint,
    token::authority = pool,
)]
    pub vault_a: Account<'info, TokenAccount>,

    #[account(
    init,
    payer = payer,
    seeds = [b"vault_b", pool.key().as_ref()],
    bump,
    token::mint = token_b_mint,
    token::authority = pool,
)]
    pub vault_b: Account<'info, TokenAccount>,

    #[account(
    init,
    payer = payer,
    seeds = [b"lp_mint", pool.key().as_ref()],
    bump,
    mint::decimals = 6,
    mint::authority = pool,
)]
    pub lp_mint: Account<'info, Mint>,

    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct RemoveLiquidity<'info> {
    #[account(mut)]
    pub user: Signer<'info>,

    #[account(
        mut,
        seeds = [b"pool", pool.token_a_mint.as_ref(), pool.token_b_mint.as_ref()],
        bump
    )]
    pub pool: Account<'info, Pool>,

    #[account(
        mut,
        seeds = [b"vault_a", pool.key().as_ref()],
        bump,
        token::mint = token_a_mint,
        token::authority = pool
    )]
    pub vault_a: Account<'info, TokenAccount>,

    #[account(
        mut,
        seeds = [b"vault_b", pool.key().as_ref()],
        bump,
        token::mint = token_b_mint,
        token::authority = pool
    )]
    pub vault_b: Account<'info, TokenAccount>,

    #[account(
        mut,
        seeds = [b"lp_mint", pool.key().as_ref()],
        bump,
        mint::authority = pool
    )]
    pub lp_mint: Account<'info, Mint>,

    #[account(
        mut,
        constraint = user_lp.owner == user.key() @ DexError::InvalidUserAccount,
        constraint = user_lp.mint == lp_mint.key() @ DexError::InvalidLpAccount
    )]
    pub user_lp: Account<'info, TokenAccount>,

    #[account(
        mut,
        constraint = user_a.owner == user.key() @ DexError::InvalidUserAccount,
        constraint = user_a.mint == token_a_mint.key() @ DexError::InvalidUserAccount
    )]
    pub user_a: Account<'info, TokenAccount>,

    #[account(
        mut,
        constraint = user_b.owner == user.key() @ DexError::InvalidUserAccount,
        constraint = user_b.mint == token_b_mint.key() @ DexError::InvalidUserAccount
    )]
    pub user_b: Account<'info, TokenAccount>,

    #[account(address = pool.token_a_mint)]
    pub token_a_mint: Account<'info, Mint>,

    #[account(address = pool.token_b_mint)]
    pub token_b_mint: Account<'info, Mint>,

    pub token_program: Program<'info, Token>,

    #[account(
        mut,
        dup,
        constraint = user_aarush_ata.owner == user.key(),
        constraint = user_aarush_ata.mint == AARUSH_MINT
    )]
    pub user_aarush_ata: Account<'info, TokenAccount>,

    #[account(
        mut,
        address = TREASURY_AARUSH_ATA,
        constraint = treasury_aarush_ata.mint == AARUSH_MINT
    )]
    pub treasury_aarush_ata: Account<'info, TokenAccount>,
}

#[derive(Accounts)]
pub struct SwapAForB<'info> {
    #[account(mut)]
    pub user: Signer<'info>,

    #[account(
        mut,
        seeds = [b"pool", pool.token_a_mint.as_ref(), pool.token_b_mint.as_ref()],
        bump
    )]
    pub pool: Account<'info, Pool>,

    #[account(
        mut,
        seeds = [b"vault_a", pool.key().as_ref()],
        bump,
        token::mint = token_a_mint,
        token::authority = pool
    )]
    pub vault_a: Account<'info, TokenAccount>,

    #[account(
        mut,
        seeds = [b"vault_b", pool.key().as_ref()],
        bump,
        token::mint = token_b_mint,
        token::authority = pool
    )]
    pub vault_b: Account<'info, TokenAccount>,

    #[account(
        mut,
        constraint = user_a.owner == user.key() @ DexError::InvalidUserAccount,
        constraint = user_a.mint == token_a_mint.key() @ DexError::InvalidUserAccount
    )]
    pub user_a: Account<'info, TokenAccount>,

    #[account(
        mut,
        constraint = user_b.owner == user.key() @ DexError::InvalidUserAccount,
        constraint = user_b.mint == token_b_mint.key() @ DexError::InvalidUserAccount
    )]
    pub user_b: Account<'info, TokenAccount>,

    #[account(address = pool.token_a_mint)]
    pub token_a_mint: Account<'info, Mint>,

    #[account(address = pool.token_b_mint)]
    pub token_b_mint: Account<'info, Mint>,

    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct SwapBForA<'info> {
    #[account(mut)]
    pub user: Signer<'info>,

    #[account(
        mut,
        seeds = [b"pool", pool.token_a_mint.as_ref(), pool.token_b_mint.as_ref()],
        bump
    )]
    pub pool: Account<'info, Pool>,

    #[account(
        mut,
        seeds = [b"vault_a", pool.key().as_ref()],
        bump,
        token::mint = token_a_mint,
        token::authority = pool
    )]
    pub vault_a: Account<'info, TokenAccount>,

    #[account(
        mut,
        seeds = [b"vault_b", pool.key().as_ref()],
        bump,
        token::mint = token_b_mint,
        token::authority = pool
    )]
    pub vault_b: Account<'info, TokenAccount>,

    #[account(
        mut,
        constraint = user_a.owner == user.key() @ DexError::InvalidUserAccount,
        constraint = user_a.mint == token_a_mint.key() @ DexError::InvalidUserAccount
    )]
    pub user_a: Account<'info, TokenAccount>,

    #[account(
        mut,
        constraint = user_b.owner == user.key() @ DexError::InvalidUserAccount,
        constraint = user_b.mint == token_b_mint.key() @ DexError::InvalidUserAccount
    )]
    pub user_b: Account<'info, TokenAccount>,

    #[account(address = pool.token_a_mint)]
    pub token_a_mint: Account<'info, Mint>,

    #[account(address = pool.token_b_mint)]
    pub token_b_mint: Account<'info, Mint>,

    pub token_program: Program<'info, Token>,
}

#[account]
pub struct Pool {
    pub token_a_mint: Pubkey,
    pub token_b_mint: Pubkey,
    pub fee_bps: u16,
    pub reserve_a: u64,
    pub reserve_b: u64,
}

impl Pool {
    pub const SIZE: usize = 32 + 32 + 2 + 8 + 8;
}

#[error_code]
pub enum DexError {
    #[msg("Invalid pool fee.")]
    InvalidFee,
    #[msg("Token A must sort before Token B.")]
    InvalidTokenOrder,
    #[msg("Amount must be greater than zero.")]
    InvalidAmount,
    #[msg("Math overflow.")]
    MathOverflow,
    #[msg("Invalid pool state.")]
    InvalidPoolState,
    #[msg("Slippage limit exceeded.")]
    SlippageExceeded,
    #[msg("Insufficient liquidity.")]
    InsufficientLiquidity,
    #[msg("LP output would be zero.")]
    InsufficientLiquidityMint,
    #[msg("LP burn would return zero.")]
    InsufficientLiquidityBurn,
    #[msg("Invalid user token account.")]
    InvalidUserAccount,
    #[msg("Invalid LP token account.")]
    InvalidLpAccount,
}
