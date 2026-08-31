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
    data = net.get_json(url)          # curl_cffi + Chrome TLS (IDX)
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


# Chrome TLS impersonation. idx.co.id fingerprints the handshake, so a plain
# requests/urllib GET is rejected before the proxy hop even matters.
IMPERSONATE = "chrome124"


def get_json(url, retries=3, timeout=30):
    """GET url with Chrome TLS impersonation, return parsed JSON.

    Used for idx.co.id, which sits behind Cloudflare. Retries with linear
    backoff and raises after the last attempt.
    """
    from curl_cffi import requests as creq

    kwargs = {"impersonate": IMPERSONATE, "timeout": timeout}
    px = proxies()
    if px:
        kwargs["proxies"] = px

    last_err = None
    for attempt in range(retries):
        try:
            r = creq.get(url, **kwargs)
            r.raise_for_status()
            return r.json()
        except Exception as e:
            last_err = e
            if attempt < retries - 1:
                time.sleep((attempt + 1) * 2)

    raise RuntimeError(f"request failed ({describe()}): {url}") from last_err
