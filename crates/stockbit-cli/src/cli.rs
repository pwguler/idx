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
    about = "Thin CLI wrapper around the Stockbit (exodus) REST API.",
    long_about = "Wraps the same endpoints the stockbit-data Python crawlers hit, but as a single \
                  stateless binary. Writes JSON to stdout — pipe into `jq` or redirect to a file.\n\n\
                  Auth: pass --token, or set STOCKBIT_BEARER_TOKEN (also read from .env)."
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
    let cfg = Config::resolve(cli.token.clone(), cli.base_url.clone())?;
    let client = Client::new(&cfg)?;
    let fmt = Format::from_pretty_flag(cli.pretty);

    let value = dispatch(&client, cli.command).await?;
    print_json(&value, fmt)
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
