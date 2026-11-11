//! Technical indicators over one ticker's daily series, matching
//! `fetch_yf_indicators.calculate_indicators` (pandas) value for value:
//! rolling windows need `n` non-missing values, EMAs are `ewm(adjust=False)`,
//! Bollinger uses the sample standard deviation, and RSI's first delta counts as
//! a zero gain/loss as pandas' `where` makes it. Non-finite results become `None`.

/// One day of input, ascending by date.
#[derive(Debug, Clone, PartialEq)]
pub struct Bar {
    pub date: String,
    pub high: Option<f64>,
    pub low: Option<f64>,
    pub close: Option<f64>,
    pub volume: f64,
}

/// Output columns, in contract order.
pub const COLUMNS: [&str; 24] = [
    "sma_5",
    "sma_10",
    "sma_20",
    "sma_50",
    "sma_200",
    "ema_12",
    "ema_26",
    "ema_50",
    "macd",
    "macd_signal",
    "macd_hist",
    "rsi_14",
    "bb_mid",
    "bb_upper",
    "bb_lower",
    "atr_14",
    "vol_sma_20",
    "vol_ratio",
    "vwap",
    "change",
    "change_5d",
    "change_20d",
    "high_20d",
    "low_20d",
];

type Series = Vec<Option<f64>>;

/// One row per bar: `(date, values in COLUMNS order)`.
#[must_use]
pub fn compute(bars: &[Bar]) -> Vec<(String, Vec<Option<f64>>)> {
    let close: Series = bars.iter().map(|b| b.close).collect();
    let high: Series = bars.iter().map(|b| b.high).collect();
    let low: Series = bars.iter().map(|b| b.low).collect();
    let volume: Series = bars.iter().map(|b| Some(b.volume)).collect();

    let ema12 = ewm(&close, 12.0);
    let ema26 = ewm(&close, 26.0);
    let macd = zip(&ema12, &ema26, |a, b| a - b);
    let macd_signal = ewm(&macd, 9.0);
    let macd_hist = zip(&macd, &macd_signal, |a, b| a - b);

    let delta: Series = (0..close.len())
        .map(|i| {
            if i == 0 {
                None
            } else {
                close[i].zip(close[i - 1]).map(|(a, b)| a - b)
            }
        })
        .collect();
    // pandas `delta.where(delta > 0, 0)`: a missing delta fails the test and becomes 0.
    let gain: Series = delta
        .iter()
        .map(|d| Some(d.filter(|d| *d > 0.0).unwrap_or(0.0)))
        .collect();
    let loss: Series = delta
        .iter()
        .map(|d| Some(-d.filter(|d| *d < 0.0).unwrap_or(0.0)))
        .collect();
    let rsi = zip(
        &rolling(&gain, 14, mean),
        &rolling(&loss, 14, mean),
        |g, l| 100.0 - 100.0 / (1.0 + g / l),
    );

    let bb_mid = rolling(&close, 20, mean);
    let bb_std = rolling(&close, 20, sample_std);
    let bb_upper = zip(&bb_mid, &bb_std, |m, s| m + 2.0 * s);
    let bb_lower = zip(&bb_mid, &bb_std, |m, s| m - 2.0 * s);

    let tr: Series = (0..close.len())
        .map(|i| {
            let prev = if i == 0 { None } else { close[i - 1] };
            [
                high[i].zip(low[i]).map(|(h, l)| h - l),
                high[i].zip(prev).map(|(h, c)| (h - c).abs()),
                low[i].zip(prev).map(|(l, c)| (l - c).abs()),
            ]
            .into_iter()
            .flatten()
            .reduce(f64::max)
        })
        .collect();
    let vol_sma = rolling(&volume, 20, mean);
    let vol_ratio = zip(&volume, &vol_sma, |v, s| v / s);
    let vwap: Series = (0..close.len())
        .map(|i| Some((high[i]? + low[i]? + close[i]?) / 3.0))
        .collect();

    let columns: Vec<Series> = vec![
        rolling(&close, 5, mean),
        rolling(&close, 10, mean),
        rolling(&close, 20, mean),
        rolling(&close, 50, mean),
        rolling(&close, 200, mean),
        ema12,
        ema26,
        ewm(&close, 50.0),
        macd,
        macd_signal,
        macd_hist,
        rsi,
        bb_mid,
        bb_upper,
        bb_lower,
        rolling(&tr, 14, mean),
        vol_sma,
        vol_ratio,
        vwap,
        pct_change(&close, 1),
        pct_change(&close, 5),
        pct_change(&close, 20),
        rolling(&high, 20, |w| {
            w.iter().copied().fold(f64::NEG_INFINITY, f64::max)
        }),
        rolling(&low, 20, |w| {
            w.iter().copied().fold(f64::INFINITY, f64::min)
        }),
    ];
    bars.iter()
        .enumerate()
        .map(|(i, b)| {
            let vals = columns
                .iter()
                .map(|c| c[i].filter(|v| v.is_finite()))
                .collect();
            (b.date.clone(), vals)
        })
        .collect()
}

fn mean(w: &[f64]) -> f64 {
    #[allow(clippy::cast_precision_loss)]
    let n = w.len() as f64;
    w.iter().sum::<f64>() / n
}

fn sample_std(w: &[f64]) -> f64 {
    let m = mean(w);
    #[allow(clippy::cast_precision_loss)]
    let n = w.len() as f64;
    (w.iter().map(|x| (x - m).powi(2)).sum::<f64>() / (n - 1.0)).sqrt()
}

/// pandas `rolling(n).agg()` with `min_periods = n`.
fn rolling(s: &Series, n: usize, agg: impl Fn(&[f64]) -> f64) -> Series {
    (0..s.len())
        .map(|i| {
            if i + 1 < n {
                return None;
            }
            let w: Option<Vec<f64>> = s[i + 1 - n..=i].iter().copied().collect();
            w.map(|w| agg(&w))
        })
        .collect()
}

/// pandas `ewm(span, adjust=False).mean()`: seeded with the first value; a
/// missing input carries the previous average forward.
fn ewm(s: &Series, span: f64) -> Series {
    let alpha = 2.0 / (span + 1.0);
    let mut prev: Option<f64> = None;
    s.iter()
        .map(|x| {
            prev = match (prev, x) {
                (None, x) => *x,
                (Some(p), Some(x)) => Some((1.0 - alpha) * p + alpha * x),
                (Some(p), None) => Some(p),
            };
            prev.filter(|_| x.is_some())
        })
        .collect()
}

fn pct_change(s: &Series, periods: usize) -> Series {
    (0..s.len())
        .map(|i| {
            let prev = s.get(i.checked_sub(periods)?)?.as_ref()?;
            Some((s[i]? / prev - 1.0) * 100.0)
        })
        .collect()
}

fn zip(a: &Series, b: &Series, f: impl Fn(f64, f64) -> f64) -> Series {
    a.iter()
        .zip(b)
        .map(|(x, y)| x.zip(*y).map(|(x, y)| f(x, y)))
        .collect()
}
