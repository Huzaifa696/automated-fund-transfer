//! automated-fund-transfer
//!
//! Daemon that keeps a configured balance on a sender keypair and transfers excess SOL
//! to a configured receiver. Sends Slack notification on successful transfer (signature included).
//!
//! Usage: automated-fund-transfer --config /etc/automated-fund-transfer/config.toml [--dry-run]

use log::LevelFilter;
use serde_json::json;
use std::{fs, path::PathBuf, time::Duration};

use anyhow::{Context, Result, anyhow};
use clap::Parser;
use solana_client::rpc_client::RpcClient;
use solana_commitment_config::CommitmentConfig;
use solana_sdk::{
    signature::{Signer, read_keypair_file},
    transaction::Transaction,
};
use solana_system_interface::instruction as system_instruction;

use tokio::time::sleep;
use tracing::{error, info, warn};

#[derive(Parser, Debug)]
struct Args {
    /// Path to the TOML config file
    #[clap(long, default_value = "/etc/automated-fund-transfer/config.toml")]
    config: String,

    /// Dry run: do not actually send transactions
    #[clap(long, action)]
    dry_run: bool,
}

/// Configuration structure for the Solana excess funds transfer service.
/// All fields are loaded from a TOML config file and some may have defaults applied.
#[derive(Debug, Clone, serde::Deserialize)]
struct Config {
    /// Path to the Solana keypair file for the sender account which is actually the validator identity.
    /// This account will be used to check the balance and send excess SOL.
    sender_keypair: String,

    /// The public key of the receiver account.
    /// All excess funds above the threshold will be transferred to this address.
    receiver_pubkey: String,

    /// Optional threshold (in SOL) above which excess funds will be transferred.
    /// If not set, defaults to `DEFAULT_SOL_THRESHOLD`.
    sol_threshold: Option<f64>,

    /// Optional polling interval in seconds.
    /// This determines how frequently the program checks the balance.
    /// Defaults to `DEFAULT_POLL_INTERVAL_SECONDS`.
    poll_interval_seconds: Option<u64>,

    /// The Solana RPC endpoint to connect to (e.g., https://api.mainnet-beta.solana.com).
    /// Used for balance checks, leader schedule, and sending transactions.
    rpc_provider: String,

    /// Optional Slack webhook URL for sending notifications.
    /// A message is sent when a threshold is exceeded and a transfer is made.
    slack_webhook: Option<String>,
}

// Define constants at the top of your module or inside an impl block if appropriate
// 1 week worth of SOLs required for voting
const DEFAULT_SOL_THRESHOLD: f64 = 7.0;

// target every 4 hrs 4*60*60 to minimize transfer fee
// cost per month = 5000 lamports fee * ((24hr / 4) * 30 days) = 900000 lamports = 0.0009 SOL = ~0.2088 $
const DEFAULT_POLL_INTERVAL_SECONDS: u64 = 14_400;

impl Config {
    fn fill_defaults(mut self) -> Self {
        if self.sol_threshold.is_none() {
            self.sol_threshold = Some(DEFAULT_SOL_THRESHOLD);
        }
        if self.poll_interval_seconds.is_none() {
            self.poll_interval_seconds = Some(DEFAULT_POLL_INTERVAL_SECONDS);
        }
        self
    }
}

async fn send_slack(webhook: &str, text: &str) -> Result<()> {
    let client = reqwest::Client::new();
    let payload = serde_json::json!({ "text": text });
    let resp = client.post(webhook).json(&payload).send().await?;
    if resp.status().is_success() {
        Ok(())
    } else {
        Err(anyhow!("slack webhook returned status {}", resp.status()))
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::new()
        .filter_level(LevelFilter::Info) // Set default level to INFO
        .format_timestamp_secs() // Optional: timestamp format
        .init();
    let args = Args::parse();

    // Load config file
    let cfg_text: String = fs::read_to_string(&args.config).context("reading config file")?;
    let cfg: Config = toml::from_str::<Config>(&cfg_text)
        .context("parsing config")?
        .fill_defaults();

    // --- Pretty-print config (redacting sensitive fields) ---
    let redacted_cfg = json!({
        "receiver_pubkey": cfg.receiver_pubkey,
        "rpc_provider": cfg.rpc_provider,
        "slack_webhook": cfg.slack_webhook,
        "sol_threshold": cfg.sol_threshold,
        "poll_interval_seconds": cfg.poll_interval_seconds,
        "sender_keypair": "[REDACTED]" // Hide sensitive path
    });

    info!(
        "Loaded configuration:\n{}",
        serde_json::to_string_pretty(&redacted_cfg).unwrap()
    );

    info!(
        "Starting automated-fund-transfer with config: {}",
        args.config
    );

    // Read keypair
    let kp_path = PathBuf::from(&cfg.sender_keypair);
    let keypair = read_keypair_file(&kp_path).map_err(|e| anyhow!("reading keypair: {}", e))?;
    let sender_pubkey = keypair.pubkey();
    info!("Loaded sender keypair: {}", sender_pubkey);

    // Parse receiver pubkey
    let receiver = cfg
        .receiver_pubkey
        .parse()
        .context("parsing receiver pubkey")?;

    // Setup RPC client
    let commitment = CommitmentConfig::finalized();
    let rpc = RpcClient::new_with_commitment(cfg.rpc_provider.clone(), commitment);

    let threshold_lamports = sol_to_lamports(cfg.sol_threshold.unwrap_or(DEFAULT_SOL_THRESHOLD));
    let poll_interval = Duration::from_secs(
        cfg.poll_interval_seconds
            .unwrap_or(DEFAULT_POLL_INTERVAL_SECONDS),
    );
    let slack_webhook = cfg.slack_webhook.clone();
    info!(
        "Configuration: threshold_sol = {}, poll_interval_s = {}",
        cfg.sol_threshold.unwrap(),
        poll_interval.as_secs(),
    );

    loop {
        // Sleep until next check. This is a simple approach. Replace with leader-slot-aware logic if desired.
        sleep(poll_interval).await;

        // Get balance
        let balance = match rpc.get_balance(&sender_pubkey) {
            Ok(b) => b,
            Err(e) => {
                warn!("Failed to get balance; will retry next loop: {}", e);
                continue;
            }
        };
        let balance_sol = lamports_to_sol(balance);
        info!(
            "Balance check: lamports = {}, sol = {}",
            balance, balance_sol
        );

        if balance > threshold_lamports {
            let excess = balance - threshold_lamports;
            let excess_sol = lamports_to_sol(excess);
            info!(
                "Excess detected; preparing transfer: excess_lamports = {}, excess_sol = {}",
                excess, excess_sol
            );

            // Build transfer
            let ix = system_instruction::transfer(&sender_pubkey, &receiver, excess);
            let recent_blockhash = match rpc.get_latest_blockhash() {
                Ok(h) => h,
                Err(e) => {
                    error!("Failed to get recent blockhash: {}", e);
                    continue;
                }
            };

            let mut tx = Transaction::new_with_payer(&[ix], Some(&sender_pubkey));
            tx.sign(&[&keypair], recent_blockhash);

            // Send and confirm transaction
            match rpc.send_and_confirm_transaction(&tx) {
                Ok(sig) => {
                    let sig_str = sig.to_string();
                    info!(
                        "Transfer confirmed: signature = {}, excess_sol = {}",
                        sig_str, excess_sol
                    );
                    // Slack notification (best-effort)
                    if let Some(webhook) = slack_webhook.as_deref() {
                        let msg = format!(
                            "Transferred {excess} Lamports from {sender} to {receiver}. Signature: {sig}",
                            excess = excess,
                            sender = sender_pubkey,
                            receiver = receiver,
                            sig = sig_str
                        );

                        // send slack (async)
                        match send_slack(webhook, &msg).await {
                            Ok(_) => info!("Slack notification sent"),
                            Err(e) => warn!("Slack notification failed: {}", e),
                        }
                    }
                }
                Err(e) => {
                    error!("Failed to send transaction: {}", e);
                }
            }
        }
    }
}

/// Number of lamports in one SOL.
pub const LAMPORTS_PER_SOL: u64 = 1_000_000_000;

/// Convert lamports (u64) to SOL (f64)
pub fn lamports_to_sol(lamports: u64) -> f64 {
    lamports as f64 / LAMPORTS_PER_SOL as f64
}

/// Convert SOL (f64) to lamports (u64)
pub fn sol_to_lamports(sol: f64) -> u64 {
    (sol * LAMPORTS_PER_SOL as f64).round() as u64
}
