//! Staging documents → contract rows for the datasets idx-mcp serves, per
//! `crates/idx-mcp/.claude/05-data-contract.md`. Every dataset has an explicit
//! column schema, so an all-null column still lands with its declared type.

use serde_json::{Map, Value, json};

use super::coerce::{abs_int, date, domicile, int, num, text, timestamp};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// `data/<name>/date=YYYY-MM-DD/*.parquet`
    TimeSeries,
    /// `data/<name>/latest.parquet`, one row per ticker.
    Snapshot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dataset {
    EodSummary,
    BrokerRankings,
    Announcements,
    BrokerActivity,
    BrokerSignal,
    Orderbook,
    BrokerDistribution,
    TradeBook,
    Fundamentals,
    YfDaily,
    Indicators,
    Companies,
    Summary,
    Analyst,
}

impl Dataset {
    pub const ALL: [Dataset; 14] = [
        Dataset::EodSummary,
        Dataset::BrokerRankings,
        Dataset::Announcements,
        Dataset::BrokerActivity,
        Dataset::BrokerSignal,
        Dataset::Orderbook,
        Dataset::BrokerDistribution,
        Dataset::TradeBook,
        Dataset::Fundamentals,
        Dataset::YfDaily,
        Dataset::Indicators,
        Dataset::Companies,
        Dataset::Summary,
        Dataset::Analyst,
    ];

    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Dataset::EodSummary => "eod_summary",
            Dataset::BrokerRankings => "broker_rankings",
            Dataset::Announcements => "announcements",
            Dataset::BrokerActivity => "broker_activity",
            Dataset::BrokerSignal => "broker_signal",
            Dataset::Orderbook => "orderbook",
            Dataset::BrokerDistribution => "broker_distribution",
            Dataset::TradeBook => "trade_book",
            Dataset::Fundamentals => "fundamentals",
            Dataset::YfDaily => "yf_daily",
            Dataset::Indicators => "indicators",
            Dataset::Companies => "companies",
            Dataset::Summary => "summary",
            Dataset::Analyst => "analyst",
        }
    }

    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|d| d.name() == name)
    }

    #[must_use]
    pub fn kind(self) -> Kind {
        match self {
            Dataset::Companies | Dataset::Summary | Dataset::Analyst => Kind::Snapshot,
            _ => Kind::TimeSeries,
        }
    }

    /// Raw-zone `(source, dataset)` inputs. Indicators derive from `yahoo/daily`.
    #[must_use]
    pub fn inputs(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Dataset::EodSummary => &[("idx", "stock_summary")],
            Dataset::BrokerRankings => &[("idx", "broker_summary")],
            Dataset::Announcements => &[("idx", "announcement"), ("idx", "news_announcement")],
            Dataset::BrokerActivity | Dataset::BrokerSignal => &[("stockbit", "marketdetectors")],
            Dataset::Orderbook => &[("stockbit", "orderbook")],
            Dataset::BrokerDistribution => &[("stockbit", "brokerdistribution")],
            Dataset::TradeBook => &[("stockbit", "tradebook")],
            Dataset::Fundamentals => &[("stockbit", "keystats")],
            Dataset::YfDaily | Dataset::Indicators => &[("yahoo", "daily")],
            Dataset::Companies => &[("stockbit", "stockprofiles")],
            Dataset::Summary => &[("yahoo", "summary")],
            Dataset::Analyst => &[("yahoo", "analyst")],
        }
    }

    /// Column name → contract type (`VARCHAR`, `DATE`, …), in output order.
    #[allow(clippy::too_many_lines)] // the column table itself
    #[must_use]
    pub fn schema(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Dataset::EodSummary => &[
                ("ticker", "VARCHAR"),
                ("date", "DATE"),
                ("stock_name", "VARCHAR"),
                ("open", "DOUBLE"),
                ("high", "DOUBLE"),
                ("low", "DOUBLE"),
                ("close", "DOUBLE"),
                ("previous", "DOUBLE"),
                ("change", "DOUBLE"),
                ("first_trade", "DOUBLE"),
                ("volume", "BIGINT"),
                ("value", "BIGINT"),
                ("frequency", "BIGINT"),
                ("bid", "DOUBLE"),
                ("offer", "DOUBLE"),
                ("bid_volume", "BIGINT"),
                ("offer_volume", "BIGINT"),
                ("foreign_buy", "BIGINT"),
                ("foreign_sell", "BIGINT"),
                ("listed_shares", "BIGINT"),
                ("tradeable_shares", "BIGINT"),
                ("weight_for_index", "BIGINT"),
                ("index_individual", "DOUBLE"),
                ("non_regular_volume", "BIGINT"),
                ("non_regular_value", "BIGINT"),
                ("non_regular_frequency", "BIGINT"),
                ("remarks", "VARCHAR"),
                ("delisting_date", "DATE"),
                ("updated_at", "TIMESTAMP"),
            ],
            Dataset::BrokerRankings => &[
                ("broker_code", "VARCHAR"),
                ("firm_name", "VARCHAR"),
                ("date", "DATE"),
                ("frequency", "BIGINT"),
                ("value", "BIGINT"),
                ("volume", "BIGINT"),
                ("rank_no", "BIGINT"),
                ("updated_at", "TIMESTAMP"),
            ],
            Dataset::Announcements => &[
                ("ticker", "VARCHAR"),
                ("date", "DATE"),
                ("source", "VARCHAR"),
                ("title", "VARCHAR"),
                ("subject", "VARCHAR"),
                ("announcement_type", "VARCHAR"),
                ("announcement_no", "VARCHAR"),
                ("published_at", "TIMESTAMP"),
                ("created_at", "TIMESTAMP"),
                ("updated_at", "TIMESTAMP"),
                ("attachments", "JSON"),
            ],
            Dataset::BrokerActivity => &[
                ("ticker", "VARCHAR"),
                ("date", "DATE"),
                ("broker_code", "VARCHAR"),
                ("side", "VARCHAR"),
                ("volume_lot", "BIGINT"),
                ("value", "BIGINT"),
                ("frequency", "BIGINT"),
                ("avg_price", "DOUBLE"),
                ("domicile", "VARCHAR"),
            ],
            Dataset::BrokerSignal => &[
                ("ticker", "VARCHAR"),
                ("date", "DATE"),
                ("accdist", "VARCHAR"),
                ("num_broker_buysell", "BIGINT"),
                ("total_buyer", "BIGINT"),
                ("total_seller", "BIGINT"),
                ("net_value", "DOUBLE"),
                ("net_percent", "DOUBLE"),
                ("net_vol", "DOUBLE"),
                ("value", "DOUBLE"),
                ("volume", "BIGINT"),
                ("average_price", "DOUBLE"),
                ("avg5", "JSON"),
                ("top1", "JSON"),
                ("top3", "JSON"),
                ("top5", "JSON"),
                ("top10", "JSON"),
            ],
            Dataset::Orderbook => &[
                ("ticker", "VARCHAR"),
                ("date", "DATE"),
                ("last_price", "DOUBLE"),
                ("open", "DOUBLE"),
                ("high", "DOUBLE"),
                ("low", "DOUBLE"),
                ("close", "DOUBLE"),
                ("previous", "DOUBLE"),
                ("change", "DOUBLE"),
                ("change_percent", "DOUBLE"),
                ("average_price", "DOUBLE"),
                ("volume", "BIGINT"),
                ("value", "BIGINT"),
                ("frequency", "BIGINT"),
                ("foreign_buy", "BIGINT"),
                ("foreign_sell", "BIGINT"),
                ("foreign_net", "BIGINT"),
                ("total_bid_lot", "BIGINT"),
                ("total_offer_lot", "BIGINT"),
                ("ara", "DOUBLE"),
                ("arb", "DOUBLE"),
                ("uma", "BOOLEAN"),
                ("bid", "JSON"),
                ("offer", "JSON"),
                ("updated_at", "TIMESTAMP"),
            ],
            Dataset::BrokerDistribution => &[
                ("ticker", "VARCHAR"),
                ("date", "DATE"),
                ("by_value", "JSON"),
                ("by_volume", "JSON"),
            ],
            Dataset::TradeBook => &[
                ("ticker", "VARCHAR"),
                ("date", "DATE"),
                ("buy_lot", "BIGINT"),
                ("sell_lot", "BIGINT"),
                ("buy_percent", "DOUBLE"),
                ("sell_percent", "DOUBLE"),
                ("buy_frequency", "BIGINT"),
                ("sell_frequency", "BIGINT"),
                ("book", "JSON"),
            ],
            Dataset::Fundamentals => &[
                ("ticker", "VARCHAR"),
                ("date", "DATE"),
                ("market_cap", "DOUBLE"),
                ("enterprise_value", "DOUBLE"),
                ("shares_outstanding", "DOUBLE"),
                ("free_float_pct", "DOUBLE"),
                ("latest_dividend", "DOUBLE"),
                ("latest_dividend_year", "BIGINT"),
                ("latest_dividend_ex_date", "DATE"),
                ("latest_dividend_payment_date", "DATE"),
                ("financial_report_currency", "JSON"),
                ("keystats_raw", "JSON"),
                ("financial_history_raw", "JSON"),
            ],
            Dataset::YfDaily => &[
                ("ticker", "VARCHAR"),
                ("date", "DATE"),
                ("open", "DOUBLE"),
                ("high", "DOUBLE"),
                ("low", "DOUBLE"),
                ("close", "DOUBLE"),
                ("volume", "BIGINT"),
                ("dividends", "DOUBLE"),
                ("splits", "DOUBLE"),
            ],
            Dataset::Indicators => &[
                ("ticker", "VARCHAR"),
                ("date", "DATE"),
                ("sma_5", "DOUBLE"),
                ("sma_10", "DOUBLE"),
                ("sma_20", "DOUBLE"),
                ("sma_50", "DOUBLE"),
                ("sma_200", "DOUBLE"),
                ("ema_12", "DOUBLE"),
                ("ema_26", "DOUBLE"),
                ("ema_50", "DOUBLE"),
                ("macd", "DOUBLE"),
                ("macd_signal", "DOUBLE"),
                ("macd_hist", "DOUBLE"),
                ("rsi_14", "DOUBLE"),
                ("bb_mid", "DOUBLE"),
                ("bb_upper", "DOUBLE"),
                ("bb_lower", "DOUBLE"),
                ("atr_14", "DOUBLE"),
                ("vol_sma_20", "DOUBLE"),
                ("vol_ratio", "DOUBLE"),
                ("vwap", "DOUBLE"),
                ("change", "DOUBLE"),
                ("change_5d", "DOUBLE"),
                ("change_20d", "DOUBLE"),
                ("high_20d", "DOUBLE"),
                ("low_20d", "DOUBLE"),
            ],
            Dataset::Companies => &[
                ("ticker", "VARCHAR"),
                ("date", "DATE"),
                ("company_name", "VARCHAR"),
                ("sector", "VARCHAR"),
                ("sub_sector", "VARCHAR"),
                ("exchange", "VARCHAR"),
                ("country", "VARCHAR"),
                ("status", "VARCHAR"),
                ("is_tradeable", "BOOLEAN"),
                ("instrument_type", "VARCHAR"),
                ("price", "DOUBLE"),
                ("previous_price", "DOUBLE"),
                ("change", "DOUBLE"),
                ("change_percent", "DOUBLE"),
                ("volume", "BIGINT"),
                ("average_price", "DOUBLE"),
                ("value", "DOUBLE"),
                ("followers", "BIGINT"),
                ("haircut_percent", "DOUBLE"),
                ("margin_percent", "DOUBLE"),
                ("company_background", "VARCHAR"),
                ("ipo_date", "DATE"),
                ("ipo_price", "DOUBLE"),
                ("listing_board", "VARCHAR"),
                ("free_float_percent", "DOUBLE"),
                ("contact_email", "VARCHAR"),
                ("secretary_name", "VARCHAR"),
                ("indexes", "JSON"),
                ("key_executive", "JSON"),
                ("listing_information", "JSON"),
            ],
            Dataset::Summary => &[
                ("ticker", "VARCHAR"),
                ("date", "DATE"),
                ("name", "VARCHAR"),
                ("business_summary", "VARCHAR"),
                ("sector", "VARCHAR"),
                ("industry", "VARCHAR"),
                ("market_cap", "DOUBLE"),
                ("enterprise_value", "DOUBLE"),
                ("shares_outstanding", "DOUBLE"),
                ("float_shares", "DOUBLE"),
                ("avg_volume", "DOUBLE"),
                ("beta", "DOUBLE"),
                ("book_value", "DOUBLE"),
                ("price_to_book", "DOUBLE"),
                ("price_to_sales", "DOUBLE"),
                ("trailing_pe", "DOUBLE"),
                ("forward_pe", "DOUBLE"),
                ("peg_ratio", "DOUBLE"),
                ("trailing_eps", "DOUBLE"),
                ("forward_eps", "DOUBLE"),
                ("revenue", "DOUBLE"),
                ("revenue_growth", "DOUBLE"),
                ("earnings_growth", "DOUBLE"),
                ("gross_margins", "DOUBLE"),
                ("operating_margins", "DOUBLE"),
                ("profit_margins", "DOUBLE"),
                ("return_on_assets", "DOUBLE"),
                ("return_on_equity", "DOUBLE"),
                ("current_ratio", "DOUBLE"),
                ("debt_to_equity", "DOUBLE"),
                ("total_debt", "DOUBLE"),
                ("total_cash", "DOUBLE"),
                ("free_cash_flow", "DOUBLE"),
                ("dividend_rate", "DOUBLE"),
                ("dividend_yield", "DOUBLE"),
                ("payout_ratio", "DOUBLE"),
                ("week_high_52", "DOUBLE"),
                ("week_low_52", "DOUBLE"),
                ("held_percent_insiders", "DOUBLE"),
                ("held_percent_institutions", "DOUBLE"),
                ("number_of_analyst_opinions", "BIGINT"),
                ("target_mean_price", "DOUBLE"),
                ("recommendation_key", "VARCHAR"),
            ],
            Dataset::Analyst => &[
                ("ticker", "VARCHAR"),
                ("date", "DATE"),
                ("rec_strong_buy", "BIGINT"),
                ("rec_buy", "BIGINT"),
                ("rec_hold", "BIGINT"),
                ("rec_sell", "BIGINT"),
                ("rec_strong_sell", "BIGINT"),
                ("target_current", "DOUBLE"),
                ("target_mean", "DOUBLE"),
                ("target_median", "DOUBLE"),
                ("target_high", "DOUBLE"),
                ("target_low", "DOUBLE"),
                ("eps_est_avg_cy", "DOUBLE"),
                ("eps_est_avg_1y", "DOUBLE"),
                ("eps_growth_1y", "DOUBLE"),
                ("rev_est_avg_cy", "DOUBLE"),
                ("rev_est_avg_1y", "DOUBLE"),
                ("earnings_estimate", "JSON"),
                ("revenue_estimate", "JSON"),
                ("eps_trend", "JSON"),
                ("eps_revisions", "JSON"),
                ("upgrades_downgrades", "JSON"),
            ],
        }
    }
}

/// Yahoo fields that arrive as fractions; the contract stores numeric percent.
const YF_PERCENT_FIELDS: [&str; 11] = [
    "revenue_growth",
    "earnings_growth",
    "gross_margins",
    "operating_margins",
    "profit_margins",
    "return_on_assets",
    "return_on_equity",
    "dividend_yield",
    "payout_ratio",
    "held_percent_insiders",
    "held_percent_institutions",
];

/// Contract rows for one staging document of `dataset`'s input `(source, raw)`.
/// `date` is the raw partition date; `fetched_at` the envelope's fetch instant.
#[must_use]
#[allow(clippy::too_many_lines)] // one field mapping per dataset, as in the contract doc
pub fn rows(dataset: Dataset, raw: &str, doc: &Value, date_: &str, fetched_at: &str) -> Vec<Value> {
    let updated_at = timestamp(&json!(fetched_at.trim_end_matches('Z')));
    let ticker = |k: &str| text(&doc[k]).map(|t| t.to_ascii_uppercase());
    let one = |pairs: Vec<(&str, Value)>| vec![obj(pairs)];
    match dataset {
        Dataset::EodSummary => one(vec![
            ("ticker", json!(ticker("StockCode"))),
            ("date", json!(date(&doc["Date"]))),
            ("stock_name", json!(text(&doc["StockName"]))),
            ("open", json!(num(&doc["OpenPrice"]))),
            ("high", json!(num(&doc["High"]))),
            ("low", json!(num(&doc["Low"]))),
            ("close", json!(num(&doc["Close"]))),
            ("previous", json!(num(&doc["Previous"]))),
            ("change", json!(num(&doc["Change"]))),
            ("first_trade", json!(num(&doc["FirstTrade"]))),
            ("volume", json!(int(&doc["Volume"]))),
            ("value", json!(int(&doc["Value"]))),
            ("frequency", json!(int(&doc["Frequency"]))),
            ("bid", json!(num(&doc["Bid"]))),
            ("offer", json!(num(&doc["Offer"]))),
            ("bid_volume", json!(int(&doc["BidVolume"]))),
            ("offer_volume", json!(int(&doc["OfferVolume"]))),
            ("foreign_buy", json!(int(&doc["ForeignBuy"]))),
            ("foreign_sell", json!(int(&doc["ForeignSell"]))),
            ("listed_shares", json!(int(&doc["ListedShares"]))),
            ("tradeable_shares", json!(int(&doc["TradebleShares"]))),
            ("weight_for_index", json!(int(&doc["WeightForIndex"]))),
            ("index_individual", json!(num(&doc["IndexIndividual"]))),
            ("non_regular_volume", json!(int(&doc["NonRegularVolume"]))),
            ("non_regular_value", json!(int(&doc["NonRegularValue"]))),
            (
                "non_regular_frequency",
                json!(int(&doc["NonRegularFrequency"])),
            ),
            ("remarks", json!(text(&doc["Remarks"]))),
            ("delisting_date", json!(date(&doc["DelistingDate"]))),
            ("updated_at", json!(updated_at)),
        ]),
        Dataset::BrokerRankings => one(vec![
            ("broker_code", json!(ticker("IDFirm"))),
            ("firm_name", json!(text(&doc["FirmName"]))),
            ("date", json!(date(&doc["Date"]))),
            ("frequency", json!(int(&doc["Frequency"]))),
            ("value", json!(int(&doc["Value"]))),
            ("volume", json!(int(&doc["Volume"]))),
            ("rank_no", json!(int(&doc["No"]))),
            ("updated_at", json!(updated_at)),
        ]),
        Dataset::Announcements if raw == "announcement" => one(vec![
            ("ticker", json!(ticker("stock_code"))),
            ("date", json!(date(&doc["Date"]))),
            ("source", json!("announcement")),
            ("title", json!(text(&doc["JudulPengumuman"]))),
            ("subject", json!(text(&doc["PerihalPengumuman"]))),
            ("announcement_type", json!(text(&doc["JenisPengumuman"]))),
            ("announcement_no", json!(text(&doc["NoPengumuman"]))),
            ("published_at", json!(timestamp(&doc["TglPengumuman"]))),
            ("created_at", json!(timestamp(&doc["CreatedDate"]))),
            ("updated_at", json!(updated_at)),
            ("attachments", doc["attachments"].clone()),
        ]),
        Dataset::Announcements => one(vec![
            ("ticker", json!(ticker("Code"))),
            ("date", json!(date(&doc["Date"]))),
            ("source", json!("news")),
            ("title", json!(text(&doc["Title"]))),
            ("subject", Value::Null),
            ("announcement_type", json!(text(&doc["Jenis"]))),
            ("announcement_no", json!(text(&doc["AnnouncementNo"]))),
            ("published_at", json!(timestamp(&doc["PublishDate"]))),
            ("created_at", Value::Null),
            ("updated_at", json!(updated_at)),
            ("attachments", doc["Attachments"].clone()),
        ]),
        Dataset::BrokerActivity => {
            let (t, d) = (ticker("stock_code"), date(&doc["date"]));
            let side = |list: &Value, side: &str, p: &str| -> Vec<Value> {
                list.as_array()
                    .into_iter()
                    .flatten()
                    .map(|b| {
                        obj(vec![
                            ("ticker", json!(t)),
                            ("date", json!(d)),
                            ("broker_code", json!(text(&b["netbs_broker_code"]))),
                            ("side", json!(side)),
                            ("volume_lot", json!(abs_int(&b[format!("{p}lot")]))),
                            ("value", json!(abs_int(&b[format!("{p}val")]))),
                            ("frequency", json!(abs_int(&b["freq"]))),
                            (
                                "avg_price",
                                json!(num(&b[if side == "B" {
                                    "netbs_buy_avg_price"
                                } else {
                                    "netbs_sell_avg_price"
                                }])),
                            ),
                            ("domicile", json!(domicile(&b["type"]))),
                        ])
                    })
                    .collect()
            };
            let s = &doc["broker_summary"];
            let mut out = side(&s["brokers_buy"], "B", "b");
            out.extend(side(&s["brokers_sell"], "S", "s"));
            out
        }
        Dataset::BrokerSignal => {
            let b = &doc["bandar_detector"];
            one(vec![
                ("ticker", json!(ticker("stock_code"))),
                ("date", json!(date(&doc["date"]))),
                ("accdist", json!(text(&b["broker_accdist"]))),
                (
                    "num_broker_buysell",
                    json!(int(&b["number_broker_buysell"])),
                ),
                ("total_buyer", json!(int(&b["total_buyer"]))),
                ("total_seller", json!(int(&b["total_seller"]))),
                ("net_value", json!(num(&b["avg"]["amount"]))),
                ("net_percent", json!(num(&b["avg"]["percent"]))),
                ("net_vol", json!(num(&b["avg"]["vol"]))),
                ("value", json!(num(&b["value"]))),
                ("volume", json!(int(&b["volume"]))),
                ("average_price", json!(num(&b["average_price"]))),
                ("avg5", b["avg5"].clone()),
                ("top1", b["top1"].clone()),
                ("top3", b["top3"].clone()),
                ("top5", b["top5"].clone()),
                ("top10", b["top10"].clone()),
            ])
        }
        Dataset::Orderbook => {
            let tbo = &doc["total_bid_offer"];
            one(vec![
                ("ticker", json!(ticker("stock_code"))),
                ("date", json!(date(&doc["date"]))),
                ("last_price", json!(num(&doc["lastprice"]))),
                ("open", json!(num(&doc["open"]))),
                ("high", json!(num(&doc["high"]))),
                ("low", json!(num(&doc["low"]))),
                ("close", json!(num(&doc["close"]))),
                ("previous", json!(num(&doc["previous"]))),
                ("change", json!(num(&doc["change"]))),
                ("change_percent", json!(num(&doc["percentage_change"]))),
                ("average_price", json!(num(&doc["average"]))),
                ("volume", json!(int(&doc["volume"]))),
                ("value", json!(int(&doc["value"]))),
                ("frequency", json!(int(&doc["frequency"]))),
                ("foreign_buy", json!(int(&doc["fbuy"]))),
                ("foreign_sell", json!(int(&doc["fsell"]))),
                ("foreign_net", json!(int(&doc["fnet"]))),
                ("total_bid_lot", json!(int(&tbo["bid"]["lot"]))),
                ("total_offer_lot", json!(int(&tbo["offer"]["lot"]))),
                ("ara", json!(num(&doc["ara"]["value"]))),
                ("arb", json!(num(&doc["arb"]["value"]))),
                ("uma", doc["uma"].as_bool().map_or(Value::Null, Value::Bool)),
                ("bid", doc["bid"].clone()),
                ("offer", doc["offer"].clone()),
                ("updated_at", json!(updated_at)),
            ])
        }
        Dataset::BrokerDistribution => one(vec![
            ("ticker", json!(ticker("stock_code"))),
            ("date", json!(date(&doc["date"]))),
            ("by_value", doc["by_value"].clone()),
            (
                "by_volume",
                if doc["by_volume"].is_null() {
                    doc["by_lot"].clone()
                } else {
                    doc["by_volume"].clone()
                },
            ),
        ]),
        Dataset::TradeBook => {
            let t = &doc["book_total"];
            one(vec![
                ("ticker", json!(ticker("stock_code"))),
                ("date", json!(date(&doc["date"]))),
                ("buy_lot", json!(int(&t["buy_lot"]))),
                ("sell_lot", json!(int(&t["sell_lot"]))),
                ("buy_percent", json!(num(&t["buy_percentage"]))),
                ("sell_percent", json!(num(&t["sell_percentage"]))),
                ("buy_frequency", json!(int(&t["buy_frequency"]))),
                ("sell_frequency", json!(int(&t["sell_frequency"]))),
                ("book", doc["book"].clone()),
            ])
        }
        Dataset::Fundamentals => {
            let stats = &doc["stats"];
            let last = doc["dividend_group"]["dividend_year_values"]
                .as_array()
                .and_then(|a| a.last())
                .cloned()
                .unwrap_or(Value::Null);
            one(vec![
                ("ticker", json!(ticker("stock_code"))),
                ("date", json!(date(&doc["date"]))),
                ("market_cap", json!(num(&stats["market_cap"]))),
                ("enterprise_value", json!(num(&stats["enterprise_value"]))),
                (
                    "shares_outstanding",
                    json!(num(&stats["current_share_outstanding"])),
                ),
                ("free_float_pct", json!(num(&stats["free_float"]))),
                ("latest_dividend", json!(num(&last["dividend"]))),
                ("latest_dividend_year", json!(int(&last["period"]))),
                ("latest_dividend_ex_date", json!(date(&last["ex_date"]))),
                (
                    "latest_dividend_payment_date",
                    json!(date(&last["payment_date"])),
                ),
                (
                    "financial_report_currency",
                    doc["financial_report_currency"].clone(),
                ),
                ("keystats_raw", doc["closure_fin_items_results"].clone()),
                (
                    "financial_history_raw",
                    doc["financial_year_parent"].clone(),
                ),
            ])
        }
        Dataset::YfDaily => one(vec![
            ("ticker", json!(ticker("ticker"))),
            ("date", json!(date(&doc["date"]))),
            ("open", json!(num(&doc["open"]))),
            ("high", json!(num(&doc["high"]))),
            ("low", json!(num(&doc["low"]))),
            ("close", json!(num(&doc["close"]))),
            ("volume", json!(int(&doc["volume"]))),
            ("dividends", json!(num(&doc["dividends"]))),
            ("splits", json!(num(&doc["splits"]))),
        ]),
        // Derived in `indicators`, not from a staging document.
        Dataset::Indicators => Vec::new(),
        Dataset::Companies => {
            let (i, p) = (&doc["info"], &doc["profile"]);
            let h = &p["history"];
            one(vec![
                ("ticker", json!(ticker("stock_code"))),
                ("date", json!(date(&doc["date"]))),
                ("company_name", json!(text(&i["name"]))),
                ("sector", json!(text(&i["sector"]))),
                ("sub_sector", json!(text(&i["sub_sector"]))),
                ("exchange", json!(text(&i["exchange"]))),
                ("country", json!(text(&i["country"]))),
                ("status", json!(text(&i["status"]))),
                (
                    "is_tradeable",
                    i["tradeable"].as_bool().map_or(Value::Null, Value::Bool),
                ),
                ("instrument_type", json!(text(&i["type_company"]))),
                ("price", json!(num(&i["price"]))),
                ("previous_price", json!(num(&i["previous"]))),
                ("change", json!(num(&i["change"]))),
                ("change_percent", json!(num(&i["percentage"]))),
                ("volume", json!(int(&i["volume"]))),
                ("average_price", json!(num(&i["average"]))),
                ("value", json!(num(&i["value"]))),
                ("followers", json!(int(&i["followers"]))),
                (
                    "haircut_percent",
                    json!(num(&i["trading_limit_info"]["haircut_percentage"])),
                ),
                (
                    "margin_percent",
                    json!(num(&i["margin_info"]["percentage_raw"])),
                ),
                ("company_background", json!(text(&p["background"]))),
                ("ipo_date", json!(date(&h["date"]))),
                ("ipo_price", json!(num(&h["price"]))),
                ("listing_board", json!(text(&h["board"]))),
                ("free_float_percent", json!(num(&h["free_float"]))),
                ("contact_email", json!(first_field(&p["address"], "email"))),
                (
                    "secretary_name",
                    json!(first_field(&p["secretary"], "name")),
                ),
                ("indexes", i["indexes"].clone()),
                ("key_executive", p["key_executive"].clone()),
                ("listing_information", p["listing_information"].clone()),
            ])
        }
        Dataset::Summary => {
            let mut m = Map::new();
            for (col, ty) in dataset.schema() {
                let v = match *col {
                    "ticker" => json!(ticker("ticker")),
                    "date" => json!(date_),
                    _ if *ty == "VARCHAR" => json!(text(&doc[*col])),
                    _ if *ty == "BIGINT" => json!(int(&doc[*col])),
                    _ if YF_PERCENT_FIELDS.contains(col) => {
                        json!(num(&doc[*col]).map(|f| f * 100.0))
                    }
                    _ => json!(num(&doc[*col])),
                };
                m.insert((*col).to_string(), v);
            }
            vec![Value::Object(m)]
        }
        Dataset::Analyst => {
            let est = |k: &str, row: &str, period: &str| json!(num(&doc[k][row][period]));
            one(vec![
                ("ticker", json!(ticker("ticker"))),
                ("date", json!(date_)),
                ("rec_strong_buy", json!(int(&doc["rec_strong_buy"]))),
                ("rec_buy", json!(int(&doc["rec_buy"]))),
                ("rec_hold", json!(int(&doc["rec_hold"]))),
                ("rec_sell", json!(int(&doc["rec_sell"]))),
                ("rec_strong_sell", json!(int(&doc["rec_strong_sell"]))),
                ("target_current", json!(num(&doc["target_current"]))),
                ("target_mean", json!(num(&doc["target_mean"]))),
                ("target_median", json!(num(&doc["target_median"]))),
                ("target_high", json!(num(&doc["target_high"]))),
                ("target_low", json!(num(&doc["target_low"]))),
                ("eps_est_avg_cy", est("earnings_estimate", "avg", "0y")),
                ("eps_est_avg_1y", est("earnings_estimate", "avg", "+1y")),
                ("eps_growth_1y", est("earnings_estimate", "growth", "+1y")),
                ("rev_est_avg_cy", est("revenue_estimate", "avg", "0y")),
                ("rev_est_avg_1y", est("revenue_estimate", "avg", "+1y")),
                ("earnings_estimate", doc["earnings_estimate"].clone()),
                ("revenue_estimate", doc["revenue_estimate"].clone()),
                ("eps_trend", doc["eps_trend"].clone()),
                ("eps_revisions", doc["eps_revisions"].clone()),
                ("upgrades_downgrades", doc["upgrades_downgrades"].clone()),
            ])
        }
    }
}

/// `field` of the first element of a list whose values may themselves be
/// JSON-encoded strings (Stockbit's profile address/secretary entries).
fn first_field(list: &Value, field: &str) -> Option<String> {
    let v = list.as_array()?.first()?.get(field)?;
    match v {
        Value::String(s) => match serde_json::from_str::<Value>(s) {
            Ok(Value::String(inner)) => text(&Value::String(inner)),
            _ => text(v),
        },
        other => text(other),
    }
}

fn obj(pairs: Vec<(&str, Value)>) -> Value {
    Value::Object(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}
