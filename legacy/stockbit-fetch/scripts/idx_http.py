"""Shared IDX HTTP client.

idx.co.id sits behind a Cloudflare TLS/JA3 fingerprint check (not a JS
challenge), so a plain requests/urllib GET gets a 403. curl_cffi with Chrome
TLS impersonation passes it without a browser. Used by the fetch_idx_* scripts.
"""

import time

from curl_cffi import requests as creq

IMPERSONATE = "chrome124"  # chrome124/chrome120 pass Cloudflare; chrome110 gets 403


def get_json(url, retries=4, timeout=25):
    """GET url with Chrome TLS impersonation, return parsed JSON. Retries on
    transient failure with linear backoff; raises after the last attempt."""
    last_err = None
    for attempt in range(retries):
        try:
            r = creq.get(url, impersonate=IMPERSONATE, timeout=timeout)
            r.raise_for_status()
            return r.json()
        except Exception as e:
            last_err = e
            time.sleep((attempt + 1) * 2)
    raise RuntimeError(f"IDX request failed after {retries} attempts: {url}") from last_err
