//! Clap-derive command surface. Each subcommand maps to exactly one Stockbit endpoint
//! and prints its JSON payload to stdout.

use std::process::ExitCode;

use clap::{Parser, Subcommand};
use serde_json::{json, Value};

use crate::api;
use crate::client::Client;
use crate::config::{Config, TOKEN_ENV};
use crate::error::Result;
use crate::output::{print_json, Format};
use crate::stored_config::StoredConfig;

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use super::Cli;

    #[test]
    fn clap_definition_is_valid() {
        // Catches subtle clap derive misconfigurations (default_value_t types,
        // global-arg conflicts, duplicate flags) at test-time instead of runtime.
        Cli::command().debug_assert();
    }
}

#[derive(Debug, Parser)]
#[command(
    name = "stockbit",
    bin_name = "stockbit",
    version,
    about = "CLI wrapper around the Stockbit (exodus) REST API.",
    long_about = "Writes raw upstream JSON to stdout — pipe into `jq` or redirect to a file.\n\n\
                  Auth (resolved in order): --token flag, $STOCKBIT_BEARER_TOKEN env var, \
                  ~/.stockbit-cli/config.yaml. Manage the stored config with \
                  `stockbit config set token <VALUE>`."
)]
pub struct Cli {
    /// Bearer token. Defaults to `$STOCKBIT_BEARER_TOKEN` or value in `.env`.
    #[arg(long, global = true, env = TOKEN_ENV, hide_env_values = true)]
    pub token: Option<String>,

    /// Override the API base URL (default: <https://exodus.stockbit.com>).
    #[arg(long, global = true)]
    pub base_url: Option<String>,

    /// Pretty-print JSON output.
    #[arg(long, short = 'p', global = true)]
    pub pretty: bool,

    /// Verbosity: -v info, -vv debug, -vvv trace.
    #[arg(short, long, global = true, action = clap::ArgAction::Count)]
    pub verbose: u8,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Key statistics ratios (PER, PBV, EPS, …) for a symbol.
    Keystats {
        symbol: String,
        /// Years of history to fetch (default: 10).
        #[arg(long, default_value_t = crate::api::keystats::DEFAULT_YEAR_LIMIT)]
        year_limit: u32,
    },
    /// Short company info: name, sector, last close.
    Info { symbol: String },
    /// Long-form company profile.
    Profile { symbol: String },
    /// Combined info + profile in one payload.
    Emitten { symbol: String },
    /// Market detectors (foreign-flow / accumulation) for one symbol and date range.
    MarketDetectors {
        symbol: String,
        /// Start date YYYY-MM-DD (optional — omit for trailing window).
        #[arg(long)]
        from: Option<String>,
        /// End date YYYY-MM-DD (optional).
        #[arg(long)]
        to: Option<String>,
        #[arg(long, default_value_t = 100)]
        limit: u32,
        #[arg(long, default_value = "TRANSACTION_TYPE_NET")]
        transaction_type: String,
        #[arg(long, default_value = "MARKET_BOARD_REGULER")]
        market_board: String,
        #[arg(long, default_value = "INVESTOR_TYPE_ALL")]
        investor_type: String,
    },
    /// Live orderbook snapshot for a symbol.
    Orderbook { symbol: String },
    /// Broker distribution for a symbol on a given date.
    BrokerDistribution {
        symbol: String,
        #[arg(long)]
        date: String,
    },
    /// Time/price-grouped trade book for a symbol on a date.
    TradeBook {
        symbol: String,
        #[arg(long)]
        date: String,
        #[arg(long, default_value = crate::api::trade_book::DEFAULT_GROUP_BY)]
        group_by: String,
        #[arg(long, default_value = crate::api::trade_book::DEFAULT_TIME_INTERVAL)]
        time_interval: String,
    },
    /// Shareholder composition snapshots across reporting periods.
    Shareholders { symbol: String },
    /// Manage the on-disk config at `~/.stockbit-cli/config.yaml`.
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
}

#[derive(Debug, Subcommand)]
pub enum ConfigAction {
    /// Print the current stored config (token shown as <set>/<unset>).
    Show,
    /// Print the on-disk config path.
    Path,
    /// Set a config value. Keys: `token`, `base-url`.
    Set { key: String, value: String },
    /// Remove a stored config value. Keys: `token`, `base-url`.
    Unset { key: String },
}

pub async fn run() -> ExitCode {
    let cli = Cli::parse();
    init_tracing(cli.verbose);

    match run_inner(cli).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(1)
        }
    }
}

async fn run_inner(cli: Cli) -> Result<()> {
    let fmt = Format::from_pretty_flag(cli.pretty);

    // The `config` subcommand never authenticates — it only reads/writes the local
    // YAML file, so we short-circuit before invoking Config::resolve (which would
    // demand a token).
    if let Command::Config { action } = cli.command {
        return run_config(action, fmt);
    }

    let cfg = Config::resolve(cli.token.clone(), cli.base_url.clone())?;
    let client = Client::new(&cfg)?;
    let value = dispatch(&client, cli.command).await?;
    print_json(&value, fmt)
}

fn run_config(action: ConfigAction, fmt: Format) -> Result<()> {
    match action {
        ConfigAction::Show => {
            let stored = StoredConfig::load()?;
            print_json(&stored.redacted(), fmt)
        }
        ConfigAction::Path => {
            println!("{}", StoredConfig::default_path()?.display());
            Ok(())
        }
        ConfigAction::Set { key, value } => {
            let mut stored = StoredConfig::load()?;
            stored.set(&key, value)?;
            let path = stored.save()?;
            eprintln!("ok ({})", path.display());
            Ok(())
        }
        ConfigAction::Unset { key } => {
            let mut stored = StoredConfig::load()?;
            let removed = stored.unset(&key)?;
            let path = stored.save()?;
            eprintln!(
                "{} ({})",
                if removed.is_some() {
                    "ok"
                } else {
                    "no-op: key not set"
                },
                path.display()
            );
            Ok(())
        }
    }
}

async fn dispatch(client: &Client, cmd: Command) -> Result<Value> {
    match cmd {
        Command::Keystats { symbol, year_limit } => {
            api::keystats::fetch(client, &symbol, year_limit).await
        }
        Command::Info { symbol } => api::info::fetch(client, &symbol).await,
        Command::Profile { symbol } => api::profile::fetch(client, &symbol).await,
        Command::Emitten { symbol } => api::emitten::fetch(client, &symbol).await,
        Command::MarketDetectors {
            symbol,
            from,
            to,
            limit,
            transaction_type,
            market_board,
            investor_type,
        } => {
            let params = api::market_detectors::Params {
                transaction_type: &transaction_type,
                market_board: &market_board,
                investor_type: &investor_type,
                limit,
                from: from.as_deref(),
                to: to.as_deref(),
            };
            api::market_detectors::fetch(client, &symbol, &params).await
        }
        Command::Orderbook { symbol } => api::orderbook::fetch(client, &symbol).await,
        Command::BrokerDistribution { symbol, date } => {
            api::broker_distribution::fetch(client, &symbol, &date).await
        }
        Command::TradeBook {
            symbol,
            date,
            group_by,
            time_interval,
        } => api::trade_book::fetch(client, &symbol, &date, &group_by, &time_interval).await,
        Command::Shareholders { symbol } => Ok(api::shareholders::fetch(client, &symbol)
            .await?
            .unwrap_or_else(|| {
                json!({"data": null, "_note": "no shareholder composition available for symbol"})
            })),
        Command::Config { .. } => unreachable!("`config` is handled in run_inner before dispatch"),
    }
}

fn init_tracing(verbosity: u8) {
    use tracing_subscriber::EnvFilter;
    let default = match verbosity {
        0 => "warn",
        1 => "info",
        2 => "debug",
        _ => "trace",
    };
    // An explicit -v/-vv/-vvv must beat an exported RUST_LOG, otherwise the very
    // users reaching for verbose flags to debug an issue would silently get warn-
    // level output. Only consult RUST_LOG when the user gave no verbosity flag.
    let filter = if verbosity > 0 {
        EnvFilter::new(default)
    } else {
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default))
    };
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_writer(std::io::stderr)
        .try_init();
}
