"""Shared IDX HTTP client.

idx.co.id sits behind Cloudflare and rejects on IP reputation: an Indonesian
residential/mobile IP passes with plain curl_cffi Chrome impersonation, while
this datacenter VPS gets 403 `cf-mitigated: challenge` on every profile.

So: send the request through a proxy when one is configured, otherwise just use
this host's own connection.

Config precedence: CLI flag > .env / environment > none.

    IDX_PROXY   proxy url, e.g. http://10.10.1.54:8080
                or socks5h://10.10.1.54:1080. Unset/empty = direct.
"""

import os
import time
from pathlib import Path

from curl_cffi import requests as creq
from dotenv import load_dotenv

load_dotenv(Path(__file__).resolve().parent.parent / ".env")

IMPERSONATE = "chrome124"

_proxy = (os.environ.get("IDX_PROXY") or "").strip()


def set_proxy(url):
    """Set the proxy for subsequent requests. Empty/None means direct."""
    global _proxy
    _proxy = (url or "").strip()
    return _proxy


def add_cli_args(parser):
    """Register --idx-proxy on an argparse parser."""
    parser.add_argument(
        "--idx-proxy",
        type=str,
        default=None,
        metavar="URL",
        help=(
            "proxy for idx.co.id, e.g. http://10.10.1.54:8080 or "
            "socks5h://10.10.1.54:1080; pass '' to go direct "
            f"(default from .env IDX_PROXY: {_proxy or 'direct'})"
        ),
    )
    return parser


def apply_cli_args(args):
    """Apply --idx-proxy if it was given. Returns the effective proxy."""
    val = getattr(args, "idx_proxy", None)
    if val is not None:
        set_proxy(val)
    return _proxy


def describe():
    return f"idx: via {_proxy}" if _proxy else "idx: direct"


def get_json(url, retries=3, timeout=30):
    """GET url and return parsed JSON, through the proxy when configured."""
    kwargs = {"impersonate": IMPERSONATE, "timeout": timeout}
    if _proxy:
        kwargs["proxies"] = {"http": _proxy, "https": _proxy}

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

    raise RuntimeError(f"IDX request failed ({describe()}): {url}") from last_err
