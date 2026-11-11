//! Raw payloads → staging documents: the exact records the Python fetchers
//! upserted into `MongoDB`. Golden tests pin each function to documents captured
//! from the Python code for the same payloads, so the contract step sees the same
//! shapes whether a record came from a fresh fetch or from the Mongo import.
//!
//! Two Python quirks are kept for parity: `yfanalyst.rec_*` counts come from the
//! last (oldest) recommendation period, and `upgrades_downgrades[].date` is null.

use serde_json::{Map, Value, json};

use crate::raw::Envelope;

/// Staging documents for one raw envelope of `(source, dataset)`, partitioned on `date`.
#[must_use]
pub fn docs(source: &str, dataset: &str, date: &str, env: &Envelope) -> Vec<Value> {
    let p = &env.payload;
    match (source, dataset) {
        ("idx", "stock_summary" | "broker_summary") => rows(&p["data"])
            .iter()
            .map(|row| {
                let mut doc = row.clone();
                let d = match &row["Date"] {
                    v if truthy(v) => Value::String(prefix(v, 10)),
                    _ => Value::String(date.to_string()),
                };
                set(&mut doc, "Date", d);
                doc
            })
            .collect(),
        ("idx", "announcement") => rows(&p["Replies"])
            .iter()
            .map(|i| announcement(i, date))
            .collect(),
        ("idx", "news_announcement") => rows(&p["Items"]).iter().map(news).collect(),
        ("stockbit", "stockprofiles") => vec![json!({
            "date": date,
            "stock_code": env.key,
            "info": or_empty(&p["info"]["data"]),
            "profile": or_empty(&p["profile"]["data"]),
        })],
        ("stockbit", _) => {
            let mut doc = or_empty(&p["data"]);
            set(&mut doc, "date", json!(date));
            set(&mut doc, "stock_code", json!(env.key));
            vec![doc]
        }
        // Imported from MongoDB (`etl::mongo`): already staging documents.
        ("yahoo", "daily") if p.get("rows").is_some() => rows(&p["rows"]),
        ("yahoo", "summary" | "analyst") if p.get("doc").is_some() => vec![p["doc"].clone()],
        ("yahoo", "daily") => yf_daily(&env.key, p),
        ("yahoo", "summary") => yf_summary(&env.key, p).into_iter().collect(),
        ("yahoo", "analyst") => yf_analyst(&env.key, p).into_iter().collect(),
        _ => Vec::new(),
    }
}

fn announcement(item: &Value, date: &str) -> Value {
    let p = &item["pengumuman"];
    let tgl = p["TglPengumuman"].as_str().unwrap_or("");
    json!({
        "Id2": p["Id2"],
        "NoPengumuman": p["NoPengumuman"],
        "stock_code": p["Kode_Emiten"].as_str().unwrap_or("").trim(),
        "Date": if tgl.is_empty() { date.to_string() } else { prefix(&json!(tgl), 10) },
        "TglPengumuman": tgl,
        "JudulPengumuman": p["JudulPengumuman"],
        "JenisPengumuman": p["JenisPengumuman"],
        "PerihalPengumuman": p["PerihalPengumuman"],
        "Form_Id": p["Form_Id"],
        "CreatedDate": p["CreatedDate"],
        "attachments": item.get("attachments").cloned().unwrap_or_else(|| json!([])),
    })
}

fn news(item: &Value) -> Value {
    let get_or = |k: &str| item.get(k).cloned().unwrap_or_else(|| json!(""));
    let publish = get_or("PublishDate");
    let attachments = match item.get("Attachments") {
        Some(Value::Array(a)) => Value::Array(a.clone()),
        None => json!([]),
        Some(_) => match &item["PdfPath"] {
            Value::String(s) if s.starts_with('[') => {
                serde_json::from_str(s).unwrap_or_else(|_| json!([]))
            }
            Value::Array(a) => Value::Array(a.clone()),
            _ => json!([]),
        },
    };
    json!({
        "Id": get_or("Id"),
        "AnnouncementNo": get_or("AnnouncementNo"),
        "PublishDate": publish,
        "Date": if truthy(&publish) { prefix(&publish, 10) } else { String::new() },
        "Title": get_or("Title"),
        "Code": item["Code"].as_str().unwrap_or("").trim(),
        "Jenis": get_or("Jenis"),
        "JmsxGroupId": get_or("JmsxGroupId"),
        "Attachments": attachments,
    })
}

/// Daily OHLCV as yfinance `history(auto_adjust=True)` returns it: prices scaled
/// by `adjclose / close`, volume unadjusted, dividends and split ratios per day.
fn yf_daily(ticker: &str, payload: &Value) -> Vec<Value> {
    let r = &payload["chart"]["result"][0];
    let Some(ts) = r["timestamp"].as_array() else {
        return Vec::new();
    };
    let offset = r["meta"]["gmtoffset"].as_i64().unwrap_or(0);
    let q = &r["indicators"]["quote"][0];
    let adj = &r["indicators"]["adjclose"][0]["adjclose"];
    let day = |secs: i64| {
        chrono::DateTime::from_timestamp(secs + offset, 0)
            .map(|t| t.date_naive().format("%Y-%m-%d").to_string())
    };
    let mut dividends = std::collections::HashMap::new();
    if let Some(m) = r["events"]["dividends"].as_object() {
        for e in m.values() {
            if let (Some(d), Some(a)) = (e["date"].as_i64().and_then(day), e["amount"].as_f64()) {
                dividends.insert(d, a);
            }
        }
    }
    let mut splits = std::collections::HashMap::new();
    if let Some(m) = r["events"]["splits"].as_object() {
        for e in m.values() {
            let ratio = e["numerator"]
                .as_f64()
                .zip(e["denominator"].as_f64())
                .map(|(n, d)| n / d);
            if let (Some(d), Some(x)) = (e["date"].as_i64().and_then(day), ratio) {
                splits.insert(d, x);
            }
        }
    }
    let mut out = Vec::with_capacity(ts.len());
    for (i, t) in ts.iter().enumerate() {
        let Some(date) = t.as_i64().and_then(day) else {
            continue;
        };
        let f = |a: &Value| a.get(i).and_then(Value::as_f64);
        let close = f(&q["close"]);
        let ratio = f(adj).zip(close).map(|(a, c)| a / c);
        let scaled = |v: Option<f64>| v.zip(ratio).map(|(v, r)| v * r);
        out.push(json!({
            "ticker": ticker,
            "date": date,
            "open": scaled(f(&q["open"])),
            "high": scaled(f(&q["high"])),
            "low": scaled(f(&q["low"])),
            "close": f(adj),
            "volume": q["volume"].get(i).and_then(Value::as_i64).unwrap_or(0),
            "dividends": dividends.get(&date).copied().unwrap_or(0.0),
            "splits": splits.get(&date).copied().unwrap_or(0.0),
        }));
    }
    out
}

/// Yahoo field → staging field, as in `fetch_yf_summary.FIELDS`.
pub const YF_SUMMARY_FIELDS: [(&str, &str); 42] = [
    ("longName", "name"),
    ("sector", "sector"),
    ("industry", "industry"),
    ("marketCap", "market_cap"),
    ("enterpriseValue", "enterprise_value"),
    ("trailingPE", "trailing_pe"),
    ("forwardPE", "forward_pe"),
    ("pegRatio", "peg_ratio"),
    ("priceToBook", "price_to_book"),
    ("priceToSalesTrailing12Months", "price_to_sales"),
    ("profitMargins", "profit_margins"),
    ("grossMargins", "gross_margins"),
    ("operatingMargins", "operating_margins"),
    ("returnOnAssets", "return_on_assets"),
    ("returnOnEquity", "return_on_equity"),
    ("totalRevenue", "revenue"),
    ("revenueGrowth", "revenue_growth"),
    ("earningsGrowth", "earnings_growth"),
    ("totalCash", "total_cash"),
    ("totalDebt", "total_debt"),
    ("debtToEquity", "debt_to_equity"),
    ("currentRatio", "current_ratio"),
    ("freeCashflow", "free_cash_flow"),
    ("beta", "beta"),
    ("fiftyTwoWeekHigh", "week_high_52"),
    ("fiftyTwoWeekLow", "week_low_52"),
    ("averageVolume", "avg_volume"),
    ("sharesOutstanding", "shares_outstanding"),
    ("floatShares", "float_shares"),
    ("heldPercentInsiders", "held_percent_insiders"),
    ("heldPercentInstitutions", "held_percent_institutions"),
    ("dividendRate", "dividend_rate"),
    ("dividendYield", "dividend_yield"),
    ("payoutRatio", "payout_ratio"),
    ("trailingEps", "trailing_eps"),
    ("forwardEps", "forward_eps"),
    ("bookValue", "book_value"),
    ("targetMeanPrice", "target_mean_price"),
    ("recommendationKey", "recommendation_key"),
    ("numberOfAnalystOpinions", "number_of_analyst_opinions"),
    ("longBusinessSummary", "business_summary"),
    ("regularMarketPrice", "_regular_market_price"),
];

/// yfinance `.info` flattening: module fields merged (the `price` module last,
/// standing in for the v7 quote that overrides quoteSummary), `{raw, fmt}`
/// reduced to `raw`, NBSP replaced in strings. `None` when Yahoo has neither a
/// price nor a market cap, as the Python job skipped those tickers.
fn yf_summary(ticker: &str, payload: &Value) -> Option<Value> {
    let result = &payload["quoteSummary"]["result"][0];
    let mut info = Map::new();
    for module in [
        "summaryDetail",
        "defaultKeyStatistics",
        "financialData",
        "assetProfile",
        "price",
    ] {
        if let Some(m) = result[module].as_object() {
            for (k, v) in m {
                if !v.is_null() {
                    info.insert(k.clone(), yf_format(v));
                }
            }
        }
    }
    let get = |k: &str| info.get(k).cloned().unwrap_or(Value::Null);
    if !(truthy(&get("regularMarketPrice")) || truthy(&get("marketCap"))) {
        return None;
    }
    let mut doc = Map::new();
    doc.insert("ticker".into(), json!(ticker));
    for (yh, ours) in YF_SUMMARY_FIELDS {
        if !ours.starts_with('_') {
            doc.insert(ours.into(), get(yh));
        }
    }
    Some(Value::Object(doc))
}

fn yf_format(v: &Value) -> Value {
    match v {
        Value::Object(m) if m.contains_key("raw") && m.contains_key("fmt") => m["raw"].clone(),
        Value::Object(m) => {
            Value::Object(m.iter().map(|(k, x)| (k.clone(), yf_format(x))).collect())
        }
        Value::Array(a) => Value::Array(a.iter().map(yf_format).collect()),
        Value::String(s) => Value::String(s.replace('\u{a0}', " ")),
        other => other.clone(),
    }
}

fn raw_num(v: &Value) -> Value {
    match v {
        Value::Object(m) if m.contains_key("raw") => m["raw"].clone(),
        other => other.clone(),
    }
}

fn yf_analyst(ticker: &str, payload: &Value) -> Option<Value> {
    let r = &payload["quoteSummary"]["result"][0];
    let mut doc = Map::new();
    doc.insert("ticker".into(), json!(ticker));
    let mut got = false;

    if let Some(latest) = r["recommendationTrend"]["trend"]
        .as_array()
        .and_then(|t| t.last())
    {
        for (yh, ours) in [
            ("strongBuy", "rec_strong_buy"),
            ("buy", "rec_buy"),
            ("hold", "rec_hold"),
            ("sell", "rec_sell"),
            ("strongSell", "rec_strong_sell"),
        ] {
            // Python's int(): truncation toward zero is the behavior being matched.
            #[allow(clippy::cast_possible_truncation)]
            let n = raw_num(&latest[yh]).as_f64().map_or(0, |f| f as i64);
            doc.insert(ours.into(), json!(n));
        }
        got = true;
    }

    if let Some(fd) = r["financialData"].as_object().filter(|m| !m.is_empty()) {
        let mut targets = Map::new();
        for (k, v) in fd {
            if let Some(rest) = k.strip_prefix("target") {
                targets.insert(
                    rest.to_lowercase().replace("price", "").trim().to_string(),
                    raw_num(v),
                );
            } else if k == "currentPrice" {
                targets.insert("current".into(), raw_num(v));
            }
        }
        for k in ["current", "low", "high", "mean", "median"] {
            doc.insert(
                format!("target_{k}"),
                targets.get(k).cloned().unwrap_or(Value::Null),
            );
        }
        got = true;
    }

    if let Some(h) = r["upgradeDowngradeHistory"]["history"]
        .as_array()
        .filter(|h| !h.is_empty())
    {
        let actions: Vec<Value> = h
            .iter()
            .take(10)
            .map(|row| {
                json!({
                    "date": Value::Null,
                    "firm": row["firm"],
                    "to_grade": row["toGrade"],
                    "from_grade": row["fromGrade"],
                    "action": row["action"],
                })
            })
            .collect();
        doc.insert("upgrades_downgrades".into(), Value::Array(actions));
        got = true;
    }

    let trend: Vec<&Value> = r["earningsTrend"]["trend"]
        .as_array()
        .map(|t| t.iter().take(4).collect())
        .unwrap_or_default();
    for (key, currency_key, ours) in [
        ("earningsEstimate", "earningsCurrency", "earnings_estimate"),
        ("revenueEstimate", "revenueCurrency", "revenue_estimate"),
        ("epsTrend", "epsTrendCurrency", "eps_trend"),
        ("epsRevisions", "epsRevisionsCurrency", "eps_revisions"),
    ] {
        if let Some(m) = periodic(&trend, key, currency_key) {
            doc.insert(ours.into(), m);
            got = true;
        }
    }
    got.then_some(Value::Object(doc))
}

/// `{column: {period: float|null}}` as Python's `_df_to_dict` over yfinance's
/// periodic frame. A currency column makes the Python float conversion raise, so
/// the whole block is dropped in that case.
fn periodic(trend: &[&Value], key: &str, currency_key: &str) -> Option<Value> {
    let mut periods = Vec::new();
    let mut columns: Vec<String> = Vec::new();
    let mut cells = std::collections::HashMap::new();
    for item in trend {
        let Some(block) = item[key].as_object() else {
            continue;
        };
        if block.get(currency_key).is_some_and(|c| !c.is_null()) {
            return None;
        }
        let period = item["period"].as_str().unwrap_or("").to_string();
        for (k, v) in block {
            let Some(o) = v.as_object().filter(|o| !o.is_empty()) else {
                continue;
            };
            if !columns.contains(k) {
                columns.push(k.clone());
            }
            cells.insert(
                (period.clone(), k.clone()),
                o.get("raw").and_then(Value::as_f64),
            );
        }
        periods.push(period);
    }
    if periods.is_empty() || columns.is_empty() {
        return None;
    }
    let mut out = Map::new();
    for c in &columns {
        let col: Map<String, Value> = periods
            .iter()
            .map(|p| {
                (
                    p.clone(),
                    json!(cells.get(&(p.clone(), c.clone())).copied().flatten()),
                )
            })
            .collect();
        out.insert(c.clone(), Value::Object(col));
    }
    Some(Value::Object(out))
}

fn rows(v: &Value) -> Vec<Value> {
    v.as_array().cloned().unwrap_or_default()
}

fn or_empty(v: &Value) -> Value {
    if v.is_object() { v.clone() } else { json!({}) }
}

fn set(doc: &mut Value, k: &str, v: Value) {
    if let Value::Object(m) = doc {
        m.insert(k.to_string(), v);
    }
}

fn prefix(v: &Value, n: usize) -> String {
    let s = v.as_str().map_or_else(|| v.to_string(), str::to_string);
    s.chars().take(n).collect()
}

/// Python truthiness for JSON values.
#[must_use]
pub fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64() != Some(0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}
