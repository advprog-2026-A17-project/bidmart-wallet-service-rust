# Wallet Profiling Report

## Scope

This profiling harness targets the wallet service critical path used by auction
bidding: reserving bidder funds and settling the hold after an auction outcome.

The profiled operations are:

- `Wallet::bid`
- `Wallet::release`
- `Wallet::convert`
- `WalletTransaction::new`
- `Money` arithmetic and balance validation

External I/O is intentionally excluded. Database updates, optimistic locking,
and Midtrans calls are important operationally, but they add network/storage
latency that would hide the wallet domain CPU cost in a flamegraph.

## Why This Hot Path

The highest-risk wallet path is the bidding funds flow. Every accepted bid from
the auction service must reserve active balance into held balance, and auction
settlement must either release the loser hold or convert the winner hold. If this
path is slow or allocation-heavy, it directly affects bidding throughput and user
perceived latency during active auctions.

Top-up and withdrawal flows are less suitable for CPU profiling because their
latency is dominated by Midtrans HTTP calls and payment-provider state.

## How To Run

```bash
cargo run --release --bin profile_wallet_bidding
```

The binary uses `pprof` and writes:

```text
target/profiling/profile_wallet_bidding.svg
```

`pprof` CPU sampling should be run on Linux/Unix targets. On Windows, use WSL or
the Linux CI/container environment so the signal-based sampler is available.

The terminal output also reports total runtime, total wallet mutations, and
average nanoseconds per wallet mutation.

## Workload

The harness creates funded buyer wallets, then repeats bid settlement cycles:

1. `bid(amount)` reserves active balance into held balance.
2. Most cycles call `release(amount)` to model losing bids.
3. Some cycles call `convert(amount)` to model winning settlement.

This keeps the CPU samples focused on the wallet code that runs in the auction
critical path.
