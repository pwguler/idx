# Fixtures

- `raw/<source>__<dataset>.json`: raw-zone envelopes (`{date, envelope}`) used as
  pipeline input. Payload shapes follow the upstream APIs; `yahoo__daily` is a real
  2024 BBRI.JK chart response.
- `staging/<collection>.json`: the documents the legacy stockbit-fetch Python
  fetchers produced for exactly those payloads. They were captured once by running
  the Python doc-building code (yfinance 1.7.0 for the Yahoo jobs) with the network
  patched to serve the raw fixtures and MongoDB replaced by a recorder; `updated_at`
  is dropped because it is a wall-clock stamp.
- `indicators/`: pandas-reference technical indicators (`fetch_yf_indicators.py`
  `calculate_indicators`) for a synthetic price series.
- `mongo/`: mongoexport JSONL samples in extended JSON.
