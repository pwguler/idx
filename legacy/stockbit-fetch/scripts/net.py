"""Shared outbound proxy config for every fetcher.

One knob for the whole pipeline: when FETCH_PROXY is set, all outbound calls
(Stockbit, IDX, Yahoo, token refresh) go through it; when it is empty, they use
this host's own connection.

Why it matters: idx.co.id rejects datacenter IPs outright (403
cf-mitigated:challenge), and Stockbit/Yahoo are friendlier to a residential
Indonesian egress too. Routing everything through one proxy keeps the whole
pipeline on a single identity instead of a mix of exit IPs.

Config precedence: CLI flag > FETCH_PROXY in .env / environment > direct.
IDX_PROXY is still honoured as a fallback name for backward compatibility.

Usage in a fetcher:

    import net
    net.add_cli_args(parser)          # registers --proxy
    args = parser.parse_args()
    net.apply_cli_args(args)
    print(net.describe())

    session = net.requests_session()  # requests, proxy applied
    net.apply_to_yfinance()           # yfinance, proxy applied
    data = net.get_json(url)          # curl_cffi + browser TLS (IDX)
"""

import os
import time
from pathlib import Path

from dotenv import load_dotenv

load_dotenv(Path(__file__).resolve().parent.parent / ".env")

# IDX_PROXY predates this module; keep reading it so older .env files still work.
_proxy = (os.environ.get("FETCH_PROXY") or os.environ.get("IDX_PROXY") or "").strip()


def set_proxy(url):
    """Set the proxy for subsequent requests. Empty/None means direct."""
    global _proxy
    _proxy = (url or "").strip()
    return _proxy


def get_proxy():
    return _proxy


def proxies():
    """Mapping for requests/curl_cffi, or None when going direct."""
    return {"http": _proxy, "https": _proxy} if _proxy else None


def describe():
    return f"net: via {_proxy}" if _proxy else "net: direct"


def add_cli_args(parser):
    """Register --proxy on an argparse parser."""
    parser.add_argument(
        "--proxy",
        type=str,
        default=None,
        metavar="URL",
        help=(
            "proxy for all outbound fetches, e.g. http://10.10.1.54:8080 or "
            "socks5h://10.10.1.54:1080; pass '' to go direct "
            f"(default from .env FETCH_PROXY: {_proxy or 'direct'})"
        ),
    )
    return parser


def apply_cli_args(args):
    """Apply --proxy (or legacy --idx-proxy) when given. Returns the proxy."""
    for attr in ("proxy", "idx_proxy"):
        val = getattr(args, attr, None)
        if val is not None:
            set_proxy(val)
            break
    return _proxy


def requests_session(session=None):
    """Return a requests session with the proxy applied."""
    import requests

    s = session or requests.Session()
    s.proxies.update(proxies() or {})
    if not _proxy:
        s.proxies.clear()
    return s


def apply_to_yfinance():
    """Point yfinance at the proxy. No-op when going direct."""
    if not _proxy:
        return False
    try:
        import yfinance as yf

        # yfinance >= 1.7 moved this to config.network; set_config still works
        # but warns, and older builds only have set_config.
        try:
            yf.config.network.proxy = _proxy
        except Exception:
            yf.set_config(proxy=_proxy)
        return True
    except Exception:
        # No usable yfinance config hook: fall back to env vars, which
        # requests picks up internally.
        os.environ["HTTP_PROXY"] = _proxy
        os.environ["HTTPS_PROXY"] = _proxy
        return True


# Browser TLS impersonation. idx.co.id fingerprints the handshake, so a plain
# requests/urllib GET is rejected before the proxy hop even matters.
#
# Measured 2026-09-29 through the Indonesian SOCKS5: the *iOS Safari* fingerprint
# is served (200 + JSON) while chrome124/131/136, safari17_0 and edge101 all get
# 403 cf-mitigated:challenge. That is why every Chrome-flavoured client failed.
# Override with IDX_IMPERSONATE when Cloudflare rotates its rules again.
IMPERSONATE = (os.environ.get("IDX_IMPERSONATE") or "safari18_0").strip()
IMPERSONATE_FALLBACKS = ("chrome131", "chrome124")

# The UA alone is not enough: idx.co.id also wants a request that looks like the
# XHR the page itself makes. Without these headers the same identity that returns
# 200 with them gets 403 cf-mitigated:challenge (measured 2026-09-29).
DEFAULT_HEADERS = {
    "accept": "application/json, text/plain, */*",
    "accept-language": "id-ID,id;q=0.9,en-US;q=0.8,en;q=0.7",
    "referer": "https://www.idx.co.id/",
}


def get_json(url, retries=3, timeout=30):
    """GET url with browser TLS impersonation, return parsed JSON.

    Used for idx.co.id, which sits behind Cloudflare: the TLS/HTTP2
    fingerprint decides whether the request is served or challenged, so this
    walks IMPERSONATE first and then IMPERSONATE_FALLBACKS. A 403 moves on to
    the next identity immediately (retrying the same one never helps); other
    failures retry with linear backoff. Raises after the last identity.
    """
    from curl_cffi import requests as creq

    kwargs = {"timeout": timeout, "headers": dict(DEFAULT_HEADERS)}
    px = proxies()
    if px:
        kwargs["proxies"] = px

    identities = (IMPERSONATE,) + tuple(
        i for i in IMPERSONATE_FALLBACKS if i != IMPERSONATE
    )

    last_err = None
    # Cloudflare challenges in bursts rather than permanently: a run of quick
    # requests can start answering 403 for every identity, then recover on its
    # own a few seconds later. Cycle the identities and wait it out instead of
    # giving up on the first bad patch.
    for cycle in range(retries):
        for imp in identities:
            try:
                r = creq.get(url, impersonate=imp, **kwargs)
                if r.status_code == 403:
                    last_err = RuntimeError(f"403 for impersonate={imp}")
                    continue  # fingerprint rejected: try the next identity
                r.raise_for_status()
                return r.json()
            except Exception as e:
                last_err = e
        if cycle < retries - 1:
            time.sleep(10 * (cycle + 1))

    raise RuntimeError(
        f"request failed ({describe()}, impersonate={identities}): {url}"
    ) from last_err
