# 🦀 Automated Fund Transfer Service

A Rust-based background service for **automatically transferring excess SOL funds** from one account to another when the balance exceeds a configured threshold.

Designed for **validators**, **staking operators**, or **automated fund managers**, the service runs continuously, checks balances at intervals, and sends transfers with verifiable transaction signatures — while safely logging operations and sending optional Slack notifications.

---

## 📁 Directory Layout

| Path | Purpose |
|------|----------|
| `/usr/local/bin/automated-fund-transfer` | Compiled Rust binary |
| `/etc/automated-fund-transfer/config.toml` | Configuration file |
| `/var/lib/automated-fund-transfer/` | Working directory |
| `/var/log/automated-fund-transfer/` | Log files written here |
| `/etc/systemd/system/automated-fund-transfer.service` | Systemd service unit |
| `/etc/logrotate.d/automated-fund-transfer` | Log rotation policy |

---

## ⚙️ Configuration

### Example `config.toml`

```toml
# Automated Fund Transfer configuration file

# Required parameters
sender_keypair = "/home/ubuntu/.config/solana/id.json"
receiver_pubkey = "5Abc1234xyzYourReceiverPubkeyHere"
rpc_provider = "http://127.0.0.1:8899"

# Optional parameters
slack_webhook = "https://hooks.slack.com/services/XXXX/YYYY/ZZZZ"
sol_threshold = 7.0
poll_interval_days = 7
```

---

## 🧱 Directory Setup

```bash
sudo mkdir -p /etc/automated-fund-transfer
sudo mkdir -p /var/lib/automated-fund-transfer
sudo mkdir -p /var/log/automated-fund-transfer

sudo cp ./target/release/automated-fund-transfer /usr/local/bin/
sudo cp ./sample_config.toml /etc/automated-fund-transfer/config.toml

sudo chown -R ubuntu:ubuntu /etc/automated-fund-transfer /var/log/automated-fund-transfer /var/lib/automated-fund-transfer
sudo chmod 750 /etc/automated-fund-transfer /var/log/automated-fund-transfer
```

---

## ⚡ Service Setup

`/etc/systemd/system/automated-fund-transfer.service`

```ini
[Unit]
Description=Automated Fund Transfer Service
After=network-online.target
Wants=network-online.target

[Service]
ExecStart=/usr/local/bin/automated-fund-transfer --config /etc/automated-fund-transfer/config.toml
User=ubuntu
Group=ubuntu
WorkingDirectory=/var/lib/automated-fund-transfer
Environment=RUST_LOG=info
StandardOutput=append:/var/log/automated-fund-transfer/service.log
StandardError=append:/var/log/automated-fund-transfer/service.log
Restart=always
RestartSec=5

[Install]
WantedBy=multi-user.target
```

---

## 🚀 Enable and Start the Service

```bash
sudo systemctl daemon-reload
sudo systemctl enable automated-fund-transfer
sudo systemctl start automated-fund-transfer
```

Check status:
```bash
sudo systemctl status automated-fund-transfer
```

View logs:
```bash
tail -f /var/log/automated-fund-transfer/service.log
```

---

## 🔁 Log Rotation

`/etc/logrotate.d/automated-fund-transfer`

```bash
/var/log/automated-fund-transfer/*.log {
    size 1G
    rotate 7
    missingok
    notifempty
    copytruncate
    nocompress
    su ubuntu ubuntu
}
```

---

## 🧠 Usage and Behavior

- Reads configuration from `config.toml`
- Validates parameters and applies defaults
- Checks sender balance periodically
- When balance > threshold:
  - Calculates excess
  - Transfers excess to receiver
  - Waits for confirmation
  - Logs and notifies via Slack (if configured)
- Runs continuously as a system service

---

## 🧩 CLI Options

```
automated-fund-transfer --help

USAGE:
    automated-fund-transfer --config <path>

FLAGS:
    --config <path>     Path to configuration file (TOML)
    -h, --help          Show help message
```

---

## 🪶 Logging

Uses the Rust `log` crate with `env_logger`.

- Default level: **INFO**
- Override: `RUST_LOG=debug ./automated-fund-transfer --config config.toml`

---

## 🧰 Troubleshooting

| Issue | Cause | Fix |
|--------|--------|-----|
| Service fails | Wrong binary or permission | Check `systemctl status` |
| Logs missing | Missing write permission | Check ownership of `/var/log/automated-fund-transfer` |
| Slack alerts fail | Bad webhook | Verify URL |
| RPC error | Node unreachable | Check `rpc_provider` |
| Keypair error | Not readable by ubuntu | `chown ubuntu:ubuntu id.json` |

## 🧱 Example Commands

```bash
cargo build --release
sudo cp target/release/automated-fund-transfer /usr/local/bin/
/usr/local/bin/automated-fund-transfer --config /etc/automated-fund-transfer/config.toml
sudo tail -f /var/log/automated-fund-transfer/service.log
```

