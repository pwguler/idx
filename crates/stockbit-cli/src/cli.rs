//! Clap-derive command surface. Each subcommand maps to exactly one Stockbit or
//! Yahoo endpoint and prints its JSON payload to stdout.

use std::process::ExitCode;

use clap::{Parser, Subcommand};
use serde_json::{Value, json};

use crate::api;
use crate::client::Client;
use crate::config::{Config, TOKEN_ENV};
use crate::error::Result;
use crate::idx::{self, IdxClient};
use crate::output::{Format, print_json};
use crate::stored_config::StoredConfig;
use crate::yahoo::{self, YahooClient};

#[derive(Debug, Parser)]
#[command(
    name = "stockbit",
    bin_name = "stockbit",
    version,
    about = "CLI for Stockbit (exodus), Yahoo Finance, and IDX market data.",
    long_about = "Writes raw upstream JSON to stdout — pipe into `jq` or redirect to a file.\n\n\
                  Stockbit auth (resolved in order): --token flag, $STOCKBIT_BEARER_TOKEN env var, \
                  ~/.stockbit-cli/config.yaml. Manage the stored config with \
                  `stockbit config set token <VALUE>`.\n\n\
                  Yahoo commands (`yf-*`) need no token — `yf-summary`/`yf-analyst` perform \
                  Yahoo's cookie+crumb handshake automatically.\n\n\
                  IDX commands (`idx-*`) need no token or cookie — they pass Cloudflare \
                  bot-fight via Chrome TLS impersonation (--idx-cookie is an optional extra)."
)]
pub struct Cli {
    /// Bearer token. Defaults to `$STOCKBIT_BEARER_TOKEN` or value in `.env`.
    #[arg(long, global = true, env = TOKEN_ENV, hide_env_values = true)]
    pub token: Option<String>,

    /// Override the API base URL (default: <https://exodus.stockbit.com>).
    #[arg(long, global = true)]
    pub base_url: Option<String>,

    /// Optional extra cookie for `idx-*` commands (rarely needed — IDX works
    /// cookie-less via Chrome TLS impersonation).
    #[arg(long, global = true, env = "IDX_COOKIE", hide_env_values = true)]
    pub idx_cookie: Option<String>,

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
    /// Yahoo Finance daily OHLCV (open chart API — no token needed).
    YfDaily {
        symbol: String,
        /// Start date YYYY-MM-DD (uses an explicit window; `to` defaults to now).
        #[arg(long)]
        from: Option<String>,
        /// End date YYYY-MM-DD (only used with `--from`).
        #[arg(long)]
        to: Option<String>,
        /// Range when no `--from` is given (e.g. `5d`, `1mo`, `1y`, `max`).
        #[arg(long, default_value = crate::yahoo::daily::DEFAULT_RANGE)]
        range: String,
    },
    /// Yahoo Finance latest quote snapshot (open chart API — no token needed).
    YfQuote { symbol: String },
    /// Yahoo Finance fundamentals & profile (performs a Yahoo crumb handshake).
    YfSummary { symbol: String },
    /// Yahoo Finance analyst data: recommendations, targets, estimates (crumb handshake).
    YfAnalyst { symbol: String },
    /// IDX daily stock summary (price/volume/foreign flow). Needs an IDX cookie.
    IdxStockSummary {
        /// Trading date YYYY-MM-DD (latest trading day if omitted).
        #[arg(long)]
        date: Option<String>,
    },
    /// IDX daily broker summary (per-broker buy/sell). Needs an IDX cookie.
    IdxBrokerSummary {
        /// Trading date YYYY-MM-DD (latest trading day if omitted).
        #[arg(long)]
        date: Option<String>,
    },
    /// Manage the on-disk config at `~/.stockbit-cli/config.yaml`.
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
}

impl Command {
    /// Yahoo commands hit a different service and need no Stockbit token.
    #[must_use]
    fn is_yahoo(&self) -> bool {
        matches!(
            self,
            Self::YfDaily { .. }
                | Self::YfQuote { .. }
                | Self::YfSummary { .. }
                | Self::YfAnalyst { .. }
        )
    }

    /// IDX commands need a `cf_clearance` cookie, not a Stockbit token.
    #[must_use]
    fn is_idx(&self) -> bool {
        matches!(
            self,
            Self::IdxStockSummary { .. } | Self::IdxBrokerSummary { .. }
        )
    }
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

    // Yahoo commands use a separate, token-less client; everything else goes
    // through the authenticated Stockbit client.
    let value = if cli.command.is_yahoo() {
        // Optional base override (set every Yahoo host to one URL) — handy for a
        // proxy/mirror, and used by the hermetic integration tests.
        let client = match std::env::var("STOCKBIT_YF_BASE_URL")
            .ok()
            .filter(|s| !s.is_empty())
        {
            Some(base) => YahooClient::with_bases(&base, &base, &base)?,
            None => YahooClient::new()?,
        };
        dispatch_yahoo(&client, cli.command).await?
    } else if cli.command.is_idx() {
        // IDX works token-less and cookie-less via Chrome TLS impersonation; an
        // optional cookie may be appended. Base overridable via STOCKBIT_IDX_BASE_URL
        // (proxy/mirror + hermetic tests).
        let cookie = cli.idx_cookie.clone();
        let base = std::env::var("STOCKBIT_IDX_BASE_URL")
            .ok()
            .filter(|s| !s.is_empty());
        let client = match base {
            Some(b) => IdxClient::with_base(&b, cookie.as_deref())?,
            None => IdxClient::with_cookie(cookie.as_deref())?,
        };
        dispatch_idx(&client, cli.command).await?
    } else {
        let cfg = Config::resolve(cli.token.clone(), cli.base_url.clone())?;
        let client = Client::new(&cfg)?;
        dispatch(&client, cli.command).await?
    };
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
        Command::YfDaily { .. }
        | Command::YfQuote { .. }
        | Command::YfSummary { .. }
        | Command::YfAnalyst { .. } => {
            unreachable!("yahoo commands are routed to dispatch_yahoo in run_inner")
        }
        Command::IdxStockSummary { .. } | Command::IdxBrokerSummary { .. } => {
            unreachable!("idx commands are routed to dispatch_idx in run_inner")
        }
        Command::Config { .. } => unreachable!("`config` is handled in run_inner before dispatch"),
    }
}

async fn dispatch_yahoo(client: &YahooClient, cmd: Command) -> Result<Value> {
    match cmd {
        Command::YfDaily {
            symbol,
            from,
            to,
            range,
        } => yahoo::daily::fetch(client, &symbol, from.as_deref(), to.as_deref(), &range).await,
        Command::YfQuote { symbol } => yahoo::quote::fetch(client, &symbol).await,
        Command::YfSummary { symbol } => yahoo::summary::fetch(client, &symbol).await,
        Command::YfAnalyst { symbol } => yahoo::analyst::fetch(client, &symbol).await,
        _ => unreachable!("dispatch_yahoo only receives yahoo commands"),
    }
}

async fn dispatch_idx(client: &IdxClient, cmd: Command) -> Result<Value> {
    match cmd {
        Command::IdxStockSummary { date } => {
            idx::stock_summary::fetch(client, date.as_deref()).await
        }
        Command::IdxBrokerSummary { date } => {
            idx::broker_summary::fetch(client, date.as_deref()).await
        }
        _ => unreachable!("dispatch_idx only receives idx commands"),
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
