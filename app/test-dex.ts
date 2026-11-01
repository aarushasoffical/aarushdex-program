import {
  Connection,
  Keypair,
  PublicKey,
  SystemProgram,
  Transaction,
} from "@solana/web3.js";
import {
  createMint,
  getOrCreateAssociatedTokenAccount,
  getAccount,
  mintTo,
  TOKEN_PROGRAM_ID,
} from "@solana/spl-token";
import { Program, AnchorProvider, Wallet } from "@anchor-lang/core";
import { BN } from "bn.js";
import fs from "fs";
import path from "path";

// ------------------------------------------------------------
// Configuration
// ------------------------------------------------------------

const RPC_URL = "https://api.devnet.solana.com";
const KEYPAIR_PATH = path.join(
  process.env.HOME || "~",
  ".config/solana/id.json",
);

const IDL_PATH = path.join(
  process.cwd(),
  "..",
  "target",
  "idl",
  "dex.json",
);

// Initial pool liquidity:
// 1,000 Token A + 1,000 Token B
const LIQUIDITY_A = 1_000_000_000n; // 1,000.000000
const LIQUIDITY_B = 1_000_000_000n; // 1,000.000000

// Swap 10 Token A
const SWAP_IN = 10_000_000n; // 10.000000

// 0.30% fee = 30 basis points
const FEE_BPS = 30;

// ------------------------------------------------------------
// Helpers
// ------------------------------------------------------------

function loadKeypair(filePath: string): Keypair {
  const secret = JSON.parse(fs.readFileSync(filePath, "utf8"));

  if (!Array.isArray(secret)) {
    throw new Error("Invalid Solana keypair JSON.");
  }

  return Keypair.fromSecretKey(Uint8Array.from(secret));
}

function formatTokenAmount(amount: bigint): string {
  const whole = amount / 1_000_000n;
  const fraction = (amount % 1_000_000n)
    .toString()
    .padStart(6, "0");

  return `${whole}.${fraction}`;
}

function calculateAmountOut(
  amountIn: bigint,
  reserveIn: bigint,
  reserveOut: bigint,
  feeBps: number,
): bigint {
  const feeFactor = 10_000n - BigInt(feeBps);
  const amountInWithFee = amountIn * feeFactor;

  const numerator = amountInWithFee * reserveOut;
  const denominator =
    reserveIn * 10_000n + amountInWithFee;

  return numerator / denominator;
}

function derivePoolPdas(
  programId: PublicKey,
  tokenA: PublicKey,
  tokenB: PublicKey,
) {
  const [pool] = PublicKey.findProgramAddressSync(
    [
      Buffer.from("pool"),
      tokenA.toBuffer(),
      tokenB.toBuffer(),
    ],
    programId,
  );

  const [vaultA] = PublicKey.findProgramAddressSync(
    [Buffer.from("vault_a"), pool.toBuffer()],
    programId,
  );

  const [vaultB] = PublicKey.findProgramAddressSync(
    [Buffer.from("vault_b"), pool.toBuffer()],
    programId,
  );

  const [lpMint] = PublicKey.findProgramAddressSync(
    [Buffer.from("lp_mint"), pool.toBuffer()],
    programId,
  );

  return {
    pool,
    vaultA,
    vaultB,
    lpMint,
  };
}

// ------------------------------------------------------------
// Main
// ------------------------------------------------------------

async function main() {
  console.log("=== DEX DEVNET TEST ===\n");

  // ----------------------------------------------------------
  // Connect
  // ----------------------------------------------------------

  const connection = new Connection(RPC_URL, "confirmed");
  const payer = loadKeypair(KEYPAIR_PATH);

  console.log("Wallet:", payer.publicKey.toBase58());

  const balance = await connection.getBalance(payer.publicKey);

  console.log(
    "SOL balance:",
    (balance / 1_000_000_000).toFixed(6),
  );

  if (balance < 100_000_000) {
    throw new Error(
      "Wallet has less than 0.1 SOL. Add Devnet SOL before testing.",
    );
  }

  // ----------------------------------------------------------
  // Anchor wallet/provider/program
  // ----------------------------------------------------------

  const wallet: Wallet = {
    publicKey: payer.publicKey,

    signTransaction: async <T extends Transaction>(tx: T): Promise<T> => {
      tx.partialSign(payer);
      return tx;
    },

    signAllTransactions: async <T extends Transaction>(
      txs: T[],
    ): Promise<T[]> => {
      for (const tx of txs) {
        tx.partialSign(payer);
      }

      return txs;
    },

    payer,
  };

  const provider = new AnchorProvider(
    connection,
    wallet,
    {
      commitment: "confirmed",
      preflightCommitment: "confirmed",
    },
  );

  const idl = JSON.parse(
    fs.readFileSync(IDL_PATH, "utf8"),
  );

  const program = new Program(idl, provider);

  console.log("Program:", program.programId.toBase58());

  // ----------------------------------------------------------
  // Create two test SPL tokens
  // ----------------------------------------------------------

  console.log("\nCreating Token A...");

  const mint1 = await createMint(
    connection,
    payer,
    payer.publicKey,
    null,
    6,
  );

  console.log("Token A candidate:", mint1.toBase58());

  console.log("\nCreating Token B...");

  const mint2 = await createMint(
    connection,
    payer,
    payer.publicKey,
    null,
    6,
  );

  console.log("Token B candidate:", mint2.toBase58());

  // ----------------------------------------------------------
  // Canonical ordering
  // ----------------------------------------------------------

  const [tokenA, tokenB] =
    Buffer.compare(
      mint1.toBuffer(),
      mint2.toBuffer(),
    ) < 0
      ? [mint1, mint2]
      : [mint2, mint1];

  console.log("\nCanonical token ordering:");
  console.log("Token A:", tokenA.toBase58());
  console.log("Token B:", tokenB.toBase58());

  // ----------------------------------------------------------
  // User token accounts
  // ----------------------------------------------------------

  console.log("\nCreating Token A account...");

  const userTokenA =
    await getOrCreateAssociatedTokenAccount(
      connection,
      payer,
      tokenA,
      payer.publicKey,
    );

  console.log(
    "User A:",
    userTokenA.address.toBase58(),
  );

  console.log("\nCreating Token B account...");

  const userTokenB =
    await getOrCreateAssociatedTokenAccount(
      connection,
      payer,
      tokenB,
      payer.publicKey,
    );

  console.log(
    "User B:",
    userTokenB.address.toBase58(),
  );

  // ----------------------------------------------------------
  // Mint test tokens
  // ----------------------------------------------------------

  const USER_INITIAL_A = 2_000_000_000n;
  const USER_INITIAL_B = 2_000_000_000n;

  console.log("\nMinting Token A...");

  await mintTo(
    connection,
    payer,
    tokenA,
    userTokenA.address,
    payer,
    USER_INITIAL_A,
  );

  console.log("Minted:", formatTokenAmount(USER_INITIAL_A));

  console.log("\nMinting Token B...");

  await mintTo(
    connection,
    payer,
    tokenB,
    userTokenB.address,
    payer,
    USER_INITIAL_B,
  );

  console.log("Minted:", formatTokenAmount(USER_INITIAL_B));

  // ----------------------------------------------------------
  // Derive pool accounts
  // ----------------------------------------------------------

  const {
    pool,
    vaultA,
    vaultB,
    lpMint,
  } = derivePoolPdas(
    program.programId,
    tokenA,
    tokenB,
  );

  console.log("\nDerived PDAs:");
  console.log("Pool:   ", pool.toBase58());
  console.log("Vault A:", vaultA.toBase58());
  console.log("Vault B:", vaultB.toBase58());
  console.log("LP Mint:", lpMint.toBase58());

  // ----------------------------------------------------------
  // Create pool
  // ----------------------------------------------------------

  console.log("\n=== CREATE POOL ===");

  const createPoolTx =
    await program.methods
      .createPool(FEE_BPS)
      .accounts({
        payer: payer.publicKey,
        pool,
        tokenAMint: tokenA,
        tokenBMint: tokenB,
        vaultA,
        vaultB,
        lpMint,
        tokenProgram: TOKEN_PROGRAM_ID,
        systemProgram: SystemProgram.programId,
      })
      .rpc();

  console.log("Create pool tx:", createPoolTx);

  // ----------------------------------------------------------
  // Create LP associated token account
  // ----------------------------------------------------------

  console.log("\nCreating LP token account...");

  const userLp =
    await getOrCreateAssociatedTokenAccount(
      connection,
      payer,
      lpMint,
      payer.publicKey,
    );

  console.log(
    "User LP:",
    userLp.address.toBase58(),
  );

  // ----------------------------------------------------------
  // Add liquidity
  // ----------------------------------------------------------

  console.log("\n=== ADD LIQUIDITY ===");

  const expectedLp =
    BigInt(
      Math.floor(
        Math.sqrt(
          Number(LIQUIDITY_A) *
          Number(LIQUIDITY_B),
        ),
      ),
    );

  const addLiquidityTx =
    await program.methods
      .addLiquidity(
        new BN(LIQUIDITY_A.toString()),
        new BN(LIQUIDITY_B.toString()),
        new BN(expectedLp.toString()),
      )
      .accounts({
        user: payer.publicKey,
        pool,
        vaultA,
        vaultB,
        lpMint,
        userA: userTokenA.address,
        userB: userTokenB.address,
        userLp: userLp.address,
        tokenAMint: tokenA,
        tokenBMint: tokenB,
        tokenProgram: TOKEN_PROGRAM_ID,
      })
      .rpc();

  console.log(
    "Add liquidity tx:",
    addLiquidityTx,
  );

  // ----------------------------------------------------------
  // Check reserves before swap
  // ----------------------------------------------------------

  let vaultAAccount =
    await getAccount(connection, vaultA);

  let vaultBAccount =
    await getAccount(connection, vaultB);

  let userAAccount =
    await getAccount(connection, userTokenA.address);

  let userBAccount =
    await getAccount(connection, userTokenB.address);

  console.log("\n=== BEFORE SWAP ===");

  console.log(
    "Vault A:",
    formatTokenAmount(vaultAAccount.amount),
  );

  console.log(
    "Vault B:",
    formatTokenAmount(vaultBAccount.amount),
  );

  console.log(
    "User A:",
    formatTokenAmount(userAAccount.amount),
  );

  console.log(
    "User B:",
    formatTokenAmount(userBAccount.amount),
  );

  // ----------------------------------------------------------
  // Calculate expected output
  // ----------------------------------------------------------

  const expectedOut = calculateAmountOut(
    SWAP_IN,
    BigInt(vaultAAccount.amount.toString()),
    BigInt(vaultBAccount.amount.toString()),
    FEE_BPS,
  );

  console.log(
    "\nExpected swap output:",
    formatTokenAmount(expectedOut),
    "Token B",
  );

  // Use expected output as the minimum.
  // The on-chain calculation should match it exactly.
  const minAmountOut = expectedOut;

  // ----------------------------------------------------------
  // Swap A -> B
  // ----------------------------------------------------------

  console.log("\n=== SWAP A -> B ===");

  const swapTx =
    await program.methods
      .swapAForB(
        new BN(SWAP_IN.toString()),
        new BN(minAmountOut.toString()),
      )
      .accounts({
        user: payer.publicKey,
        pool,
        vaultA,
        vaultB,
        userA: userTokenA.address,
        userB: userTokenB.address,
        tokenAMint: tokenA,
        tokenBMint: tokenB,
        tokenProgram: TOKEN_PROGRAM_ID,
      })
      .rpc();

  console.log("Swap tx:", swapTx);

  // ----------------------------------------------------------
  // Check balances after swap
  // ----------------------------------------------------------

  vaultAAccount =
    await getAccount(connection, vaultA);

  vaultBAccount =
    await getAccount(connection, vaultB);

  userAAccount =
    await getAccount(connection, userTokenA.address);

  userBAccount =
    await getAccount(connection, userTokenB.address);

  console.log("\n=== AFTER SWAP ===");

  console.log(
    "Vault A:",
    formatTokenAmount(vaultAAccount.amount),
  );

  console.log(
    "Vault B:",
    formatTokenAmount(vaultBAccount.amount),
  );

  console.log(
    "User A:",
    formatTokenAmount(userAAccount.amount),
  );

  console.log(
    "User B:",
    formatTokenAmount(userBAccount.amount),
  );

  // ----------------------------------------------------------
  // Verify expected changes
  // ----------------------------------------------------------

  console.log("\n=== VERIFICATION ===");

  const expectedUserA =
    USER_INITIAL_A -
    LIQUIDITY_A -
    SWAP_IN;

  const expectedUserB =
    USER_INITIAL_B -
    LIQUIDITY_B +
    expectedOut;

  const expectedVaultA =
    LIQUIDITY_A +
    SWAP_IN;

  const expectedVaultB =
    LIQUIDITY_B -
    expectedOut;

  const actualUserA =
    BigInt(userAAccount.amount.toString());

  const actualUserB =
    BigInt(userBAccount.amount.toString());

  const actualVaultA =
    BigInt(vaultAAccount.amount.toString());

  const actualVaultB =
    BigInt(vaultBAccount.amount.toString());

  const checks = [
    ["User Token A", actualUserA, expectedUserA],
    ["User Token B", actualUserB, expectedUserB],
    ["Vault Token A", actualVaultA, expectedVaultA],
    ["Vault Token B", actualVaultB, expectedVaultB],
  ] as const;

  let passed = true;

  for (const [name, actual, expected] of checks) {
    const ok = actual === expected;

    console.log(
      `${ok ? "PASS" : "FAIL"} ${name}`,
    );

    if (!ok) {
      console.log(
        "  expected:",
        formatTokenAmount(expected),
      );

      console.log(
        "  actual:  ",
        formatTokenAmount(actual),
      );

      passed = false;
    }
  }

  if (!passed) {
    throw new Error(
      "One or more balance checks failed.",
    );
  }

  console.log("\n================================");
  console.log("DEX SWAP TEST PASSED");
  console.log("================================");
  console.log("\nTransactions:");
  console.log("Create pool:", createPoolTx);
  console.log("Add liquidity:", addLiquidityTx);
  console.log("Swap:", swapTx);

  console.log("\nPool:");
  console.log(pool.toBase58());

  console.log("\nToken A:");
  console.log(tokenA.toBase58());

  console.log("\nToken B:");
  console.log(tokenB.toBase58());

  console.log("\nLP mint:");
  console.log(lpMint.toBase58());
}

main().catch((error) => {
  console.error("\nTEST FAILED");
  console.error(error);
  process.exit(1);
});
