//! Command surface. Global paths locate the raw zone, the Parquet output, and the
//! reference files (holiday calendar, ticker universe; embedded copies by default).

use std::path::PathBuf;
use std::process::ExitCode;

use std::time::Duration;

use anyhow::{Context, Result, bail};
use chrono::NaiveDate;
use clap::{Parser, Subcommand, ValueEnum};
use stockbit_cli::client::Client;
use stockbit_cli::config::{Config, DEFAULT_BASE_URL};
use stockbit_cli::idx::IdxClient;
use stockbit_cli::token_store::{self, TokenStore};
use stockbit_cli::yahoo::YahooClient;

use crate::calendar::Calendar;
use crate::etl::Dataset;
use crate::fetch::{self, Clients, Ctx, Job, Summary};
use crate::raw::RawStore;

const EMBEDDED_STOCKLIST: &str = include_str!("../ref/stocklist.txt");

#[derive(Debug, Parser)]
#[command(
    name = "pipeline",
    version,
    about = "Fetch IDX/Stockbit/Yahoo data into raw/ and build data/ Parquet."
)]
pub struct Cli {
    /// Raw zone root (zstd NDJSON payloads).
    #[arg(long, global = true, default_value = "raw")]
    pub raw_dir: PathBuf,
    /// Parquet output root read by idx-mcp.
    #[arg(long, global = true, default_value = "data")]
    pub data_dir: PathBuf,
    /// Holiday calendar file (one YYYY-MM-DD per line, `#` comments). Default: embedded copy.
    #[arg(long, global = true)]
    pub holidays: Option<PathBuf>,
    /// Ticker universe file (one ticker per line). Default: embedded copy.
    #[arg(long, global = true)]
    pub stocklist: Option<PathBuf>,
    /// Stockbit API base URL (token refresh and Stockbit jobs).
    #[arg(long, global = true, env = "STOCKBIT_BASE_URL")]
    pub base_url: Option<String>,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Fetch upstream payloads into the raw zone.
    Fetch(FetchArgs),
    /// Transform the raw zone into contract Parquet.
    Etl(EtlArgs),
    /// Manage the rotating Stockbit token store.
    Token {
        #[command(subcommand)]
        action: TokenAction,
    },
    /// Probe Stockbit for exchange holidays and merge them into the holiday file.
    Calendar(CalendarArgs),
    /// Fetch whole years of history, newest year first.
    Backfill(BackfillArgs),
}

#[derive(Debug, clap::Args)]
pub struct FetchArgs {
    /// Jobs to run, or `all`.
    #[arg(required = true, value_enum)]
    pub jobs: Vec<JobArg>,
    /// First date of a range (YYYY-MM-DD). With --end-date; default: today's trading date.
    #[arg(long, requires = "end_date")]
    pub start_date: Option<String>,
    /// Last date of a range (YYYY-MM-DD).
    #[arg(long, requires = "start_date")]
    pub end_date: Option<String>,
    /// Concurrent Stockbit/Yahoo requests.
    #[arg(long, default_value_t = 1)]
    pub workers: usize,
    /// Stockbit requests per second, shared by all workers.
    #[arg(long, default_value_t = 8.0)]
    pub rps: f64,
    /// Skip keys whose raw file already exists (fills gaps without refetching).
    #[arg(long)]
    pub missing_only: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum JobArg {
    All,
    IdxStockSummary,
    IdxBrokerSummary,
    IdxAnnouncement,
    IdxNews,
    SbMarketDetectors,
    SbOrderbook,
    SbBrokerDistribution,
    SbKeystats,
    SbStockProfiles,
    SbTradeBook,
    YfDaily,
    YfSummary,
    YfAnalyst,
}

#[derive(Debug, clap::Args)]
#[command(args_conflicts_with_subcommands = true)]
pub struct EtlArgs {
    /// Datasets to build (default: all).
    pub datasets: Vec<String>,
    #[command(subcommand)]
    pub action: Option<EtlAction>,
}

#[derive(Debug, Subcommand)]
pub enum EtlAction {
    /// Fold a mongoexport JSONL dump directory (`<collection>.json[l]`) into the raw zone.
    ImportMongo { dir: PathBuf },
}

#[derive(Debug, Subcommand)]
pub enum TokenAction {
    /// Rotate a refresh token from a fresh login and store the new pair.
    Seed { refresh_token: String },
    /// Rotate the stored refresh token.
    Refresh,
    /// Print the store's expiry metadata (never the tokens).
    Show,
}

#[derive(Debug, clap::Args)]
pub struct CalendarArgs {
    #[arg(long)]
    pub from: i32,
    #[arg(long)]
    pub to: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Source {
    Idx,
    Stockbit,
}

#[derive(Debug, clap::Args)]
pub struct BackfillArgs {
    #[arg(long, value_enum)]
    pub source: Source,
    #[arg(long)]
    pub from: i32,
    #[arg(long)]
    pub to: i32,
    #[arg(long, default_value_t = 5)]
    pub workers: usize,
}

pub async fn run() -> ExitCode {
    let _ = tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .try_init();
    match run_inner(Cli::parse()).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

#[allow(clippy::too_many_lines)] // one arm per command; each is short
async fn run_inner(cli: Cli) -> Result<()> {
    let raw = RawStore::new(&cli.raw_dir);
    match &cli.command {
        Command::Fetch(args) => {
            let range = match (&args.start_date, &args.end_date) {
                (Some(a), Some(b)) => Some((parse_date(a)?, parse_date(b)?)),
                _ => None,
            };
            let ctx = context(&cli, raw, range, args.workers, args.rps, args.missing_only)?;
            let jobs: Vec<Job> = if args.jobs.contains(&JobArg::All) {
                Job::ALL.to_vec()
            } else {
                args.jobs.iter().filter_map(|j| j.job()).collect()
            };
            run_jobs(&ctx, &jobs).await
        }
        Command::Etl(args) => match &args.action {
            Some(EtlAction::ImportMongo { dir }) => {
                for (collection, c) in crate::etl::mongo::import(dir, &raw)? {
                    eprintln!(
                        "{collection:22} written={} skipped={} dropped={}",
                        c.written, c.skipped, c.dropped
                    );
                }
                Ok(())
            }
            None => {
                let datasets = if args.datasets.is_empty() {
                    Dataset::ALL.to_vec()
                } else {
                    args.datasets
                        .iter()
                        .map(|n| {
                            Dataset::from_name(n).with_context(|| {
                                let names: Vec<_> = Dataset::ALL.iter().map(|d| d.name()).collect();
                                format!(
                                    "unknown dataset {n:?}; expected one of {}",
                                    names.join(", ")
                                )
                            })
                        })
                        .collect::<Result<_>>()?
                };
                let (raw, data) = (raw.clone(), cli.data_dir.clone());
                let report =
                    tokio::task::spawn_blocking(move || crate::etl::run(&raw, &data, &datasets))
                        .await??;
                for (ds, n) in report {
                    eprintln!("{:22} rows={n}", ds.name());
                }
                Ok(())
            }
        },
        Command::Token { action } => {
            let store = TokenStore::default_path()?;
            let base = cli
                .base_url
                .clone()
                .unwrap_or_else(|| DEFAULT_BASE_URL.to_string());
            let status = match action {
                TokenAction::Seed { refresh_token } => {
                    token_store::seed(&store, &base, refresh_token)
                        .await?
                        .status()
                }
                TokenAction::Refresh => token_store::refresh(&store, &base).await?.status(),
                TokenAction::Show => TokenStore::load_from(&store)?
                    .with_context(|| format!("no token store at {}", store.display()))?
                    .status(),
            };
            println!("{}", serde_json::to_string_pretty(&status)?);
            Ok(())
        }
        Command::Calendar(args) => {
            let path = cli
                .holidays
                .clone()
                .context("`pipeline calendar` writes the holiday file: pass --holidays <PATH>")?;
            let client = stockbit_client(&cli)?.context("no Stockbit token for the probe")?;
            let cache = path.with_file_name("calendar_cache");
            let mut found = Vec::new();
            for year in args.from..=args.to {
                let cached = cache.join(format!("{year}.json"));
                let holidays: Vec<String> = if let Ok(text) = std::fs::read_to_string(&cached) {
                    serde_json::from_str(&text)?
                } else {
                    let (h, trading) =
                        crate::calendar::probe_year(&client, year, Duration::from_millis(250))
                            .await?;
                    let h: Vec<String> =
                        h.iter().map(|d| d.format("%Y-%m-%d").to_string()).collect();
                    std::fs::create_dir_all(&cache)?;
                    std::fs::write(&cached, serde_json::to_string(&h)?)?;
                    eprintln!("{year}: {trading} trading days, {} holidays", h.len());
                    h
                };
                for d in holidays {
                    found.push(parse_date(&d)?);
                }
            }
            let merged = calendar(&cli)?.merged_with(&found);
            std::fs::write(&path, merged.to_file_text("IDX exchange holidays; probed years come from `pipeline calendar` (BBRI trading-day probe)."))?;
            eprintln!("wrote {}", path.display());
            Ok(())
        }
        Command::Backfill(args) => {
            let mut ctx = context(&cli, raw, None, args.workers, 8.0, false)?;
            let today = ctx
                .now
                .with_timezone(&crate::calendar::jakarta())
                .date_naive();
            let mut failed = false;
            for step in crate::backfill::plan(args.source, args.from, args.to, today) {
                eprintln!("== {:?} {}..{}", step.job, step.start, step.end);
                ctx.range = Some((step.start, step.end));
                failed |= report(step.job, fetch::run(step.job, &ctx).await);
            }
            if failed {
                bail!("backfill finished with failures");
            }
            Ok(())
        }
    }
}

async fn run_jobs(ctx: &Ctx, jobs: &[Job]) -> Result<()> {
    let mut failed = false;
    for job in jobs {
        failed |= report(*job, fetch::run(*job, ctx).await);
    }
    if failed {
        bail!("some fetches failed; rerun with --missing-only to fill the gaps");
    }
    Ok(())
}

/// Print a job's summary; `true` when anything failed.
fn report(job: Job, result: Result<Summary>) -> bool {
    match result {
        Ok(s) => {
            eprintln!(
                "{job:?}: written={} empty={} throttled={} skipped={} failed={}",
                s.written, s.empty, s.throttled, s.skipped, s.failed
            );
            s.failed > 0
        }
        Err(e) => {
            eprintln!("{job:?}: error: {e:#}");
            true
        }
    }
}

fn context(
    cli: &Cli,
    raw: RawStore,
    range: Option<(NaiveDate, NaiveDate)>,
    workers: usize,
    rps: f64,
    missing_only: bool,
) -> Result<Ctx> {
    let env_base = |k: &str| std::env::var(k).ok().filter(|s| !s.is_empty());
    Ok(Ctx {
        raw,
        calendar: calendar(cli)?,
        stocks: stocklist(cli)?,
        now: chrono::Utc::now().fixed_offset(),
        range,
        workers,
        rps,
        missing_only,
        idx_spacing: fetch::IDX_SPACING,
        backoff_unit: Duration::from_secs(1),
        clients: Clients {
            stockbit: stockbit_client(cli)?,
            idx: match env_base("STOCKBIT_IDX_BASE_URL") {
                Some(b) => IdxClient::with_base(&b, None)?,
                None => IdxClient::new()?,
            },
            yahoo: match env_base("STOCKBIT_YF_BASE_URL") {
                Some(b) => YahooClient::with_bases(&b, &b, &b)?,
                None => YahooClient::new()?,
            },
        },
    })
}

/// The CLI's own token resolution (env > shared store > config.yaml);
/// `None` when no token is configured.
fn stockbit_client(cli: &Cli) -> Result<Option<Client>> {
    match Config::resolve(None, cli.base_url.clone()) {
        Ok(cfg) => Ok(Some(Client::new(&cfg)?)),
        Err(stockbit_cli::error::Error::MissingToken) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

fn calendar(cli: &Cli) -> Result<Calendar> {
    match &cli.holidays {
        Some(p) if p.exists() => Calendar::parse(&std::fs::read_to_string(p)?),
        _ => Ok(Calendar::embedded()),
    }
}

fn stocklist(cli: &Cli) -> Result<Vec<String>> {
    let text = match &cli.stocklist {
        Some(p) => std::fs::read_to_string(p).with_context(|| format!("read {}", p.display()))?,
        None => EMBEDDED_STOCKLIST.to_string(),
    };
    Ok(text
        .lines()
        .map(|l| l.trim().to_ascii_uppercase())
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect())
}

fn parse_date(s: &str) -> Result<NaiveDate> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .with_context(|| format!("invalid date {s:?}: expected YYYY-MM-DD"))
}

impl JobArg {
    fn job(self) -> Option<Job> {
        Some(match self {
            JobArg::All => return None,
            JobArg::IdxStockSummary => Job::IdxStockSummary,
            JobArg::IdxBrokerSummary => Job::IdxBrokerSummary,
            JobArg::IdxAnnouncement => Job::IdxAnnouncement,
            JobArg::IdxNews => Job::IdxNews,
            JobArg::SbMarketDetectors => Job::SbMarketDetectors,
            JobArg::SbOrderbook => Job::SbOrderbook,
            JobArg::SbBrokerDistribution => Job::SbBrokerDistribution,
            JobArg::SbKeystats => Job::SbKeystats,
            JobArg::SbStockProfiles => Job::SbStockProfiles,
            JobArg::SbTradeBook => Job::SbTradeBook,
            JobArg::YfDaily => Job::YfDaily,
            JobArg::YfSummary => Job::YfSummary,
            JobArg::YfAnalyst => Job::YfAnalyst,
        })
    }
}
