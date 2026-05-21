/// Standalone binary for profiling the wallet bidding hot path with pprof.
///
/// Usage:
///   cargo run --release --bin profile_wallet_bidding
///
/// Output:
///   target/profiling/profile_wallet_bidding.svg
///
/// This intentionally profiles the domain layer in a tight loop. The critical
/// wallet path for BidMart is bidder fund reservation and settlement:
/// `bid` moves active balance into held balance, then an auction outcome either
/// `release`s losing holds or `convert`s the winner hold.
use bidmart_wallet_service_rust::wallet::{Money, Wallet};

#[cfg_attr(all(not(test), not(unix)), allow(dead_code))]
fn run_profile(iterations: u64, bids_per_iteration: u64) -> u64 {
    let mut transactions = 0u64;

    for user_index in 0..iterations {
        let mut wallet = Wallet::new(&format!("buyer-{user_index}"), "BUYER");
        wallet
            .top_up(Money::from_cents(bids_per_iteration * 2_000))
            .expect("profile wallet should start funded");

        for bid_index in 0..bids_per_iteration {
            let amount = Money::from_cents(1_000 + (bid_index % 25) * 100);
            wallet
                .bid(amount)
                .expect("profile bid should have enough active balance");
            transactions += 1;

            if bid_index % 10 == 0 {
                wallet
                    .convert(amount)
                    .expect("profile conversion should have enough held balance");
            } else {
                wallet
                    .release(amount)
                    .expect("profile release should have enough held balance");
            }
            transactions += 1;
        }

        std::hint::black_box(wallet);
    }

    transactions
}

#[cfg(all(not(test), unix))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use std::fs::File;
    use std::path::Path;

    use pprof::ProfilerGuardBuilder;

    let iterations = 25_000u64;
    let bids_per_iteration = 80u64;
    let output_dir = Path::new("target").join("profiling");
    let output_path = output_dir.join("profile_wallet_bidding.svg");

    std::fs::create_dir_all(&output_dir)?;

    eprintln!(
        "[profile_wallet_bidding] Starting: {} wallets x {} bid settlement cycles = {} wallet mutations",
        iterations,
        bids_per_iteration,
        iterations * bids_per_iteration * 2,
    );

    let guard = ProfilerGuardBuilder::default()
        .frequency(1000)
        .blocklist(&["libc", "libgcc", "pthread", "vdso"])
        .build()?;

    let start = std::time::Instant::now();
    let transactions = run_profile(iterations, bids_per_iteration);
    let elapsed = start.elapsed();

    if let Ok(report) = guard.report().build() {
        let file_svg = File::create(&output_path)?;
        report.flamegraph(file_svg)?;
        let raw_txt_path = output_dir.join("profile_wallet_bidding.txt");
        let mut file_txt = File::create(&raw_txt_path)?;

        for (frames, count) in &report.data {
            let mut frame_names = Vec::new();

            for symbols in &frames.frames {
                for symbol in symbols {
                    frame_names.push(symbol.name());
                }
            }

            let frame_string = frame_names.join(";");

            use std::io::Write;
            writeln!(file_txt, "{} {}", frame_string, count)?;
        }

        eprintln!(
            "[profile_wallet_bidding] Raw collapsed profile data: {}",
            raw_txt_path.display()
        );
    }

    let ns_per_transaction = elapsed.as_nanos() / transactions as u128;
    eprintln!("[profile_wallet_bidding] Done.");
    eprintln!(
        "[profile_wallet_bidding] Total time:          {:?}",
        elapsed
    );
    eprintln!(
        "[profile_wallet_bidding] Wallet mutations:     {}",
        transactions
    );
    eprintln!(
        "[profile_wallet_bidding] Per wallet mutation:  {} ns",
        ns_per_transaction
    );
    eprintln!(
        "[profile_wallet_bidding] Flamegraph:           {}",
        output_path.display()
    );

    Ok(())
}

#[cfg(all(not(test), not(unix)))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    eprintln!(
        "[profile_wallet_bidding] pprof CPU sampling is only enabled on Unix/Linux targets. Run this in Linux/WSL/CI to generate the flamegraph."
    );
    Ok(())
}

#[cfg(test)]
fn main() {}

#[cfg(test)]
mod tests {
    use super::run_profile;

    #[test]
    fn run_profile_counts_bid_and_settlement_mutations() {
        assert_eq!(run_profile(2, 5), 20);
    }
}
