#!/usr/bin/env python3
"""Send Telegram notification for fetch script results via openclaw"""
import argparse
import os
import subprocess
import sys


def send_via_openclaw(msg: str, target: str):
    result = subprocess.run(
        ["/usr/bin/openclaw", "message", "send",
         "--channel", "telegram",
         "--target", target,
         "--message", msg],
        capture_output=True, text=True
    )
    if result.returncode == 0:
        print(f"notification sent: {result.stdout.strip()}")
    else:
        print(f"notification failed: {result.stderr.strip()}")
        sys.exit(1)


def main():
    parser = argparse.ArgumentParser(description="send fetch notification via openclaw")
    parser.add_argument("--status", type=str, required=True, help="SUCCESS or FAILED")
    parser.add_argument("--scripts", type=str, help="scripts that were run")
    parser.add_argument("--success", type=int, default=0, help="number of successful scripts")
    parser.add_argument("--failed", type=int, default=0, help="number of failed scripts")
    parser.add_argument("--error", type=str, help="failed script names")
    parser.add_argument("--duration", type=str, help="total duration")
    parser.add_argument(
        "--target",
        type=str,
        default=os.getenv("TG_CHAT"),
        help="telegram chat id (defaults to TG_CHAT env var)",
    )

    args = parser.parse_args()
    if not args.target:
        sys.exit("no target: pass --target or set TG_CHAT env var")
    scripts = args.scripts or "all"

    if args.status == "SUCCESS":
        msg = f"Stockbit Fetch: {args.status}\n\nScripts: {scripts}\nResult: {args.success}/{args.success} passed"
    else:
        total = args.success + args.failed
        msg = f"Stockbit Fetch: {args.status}\n\nScripts: {scripts}\nResult: {args.success}/{total} passed\nFailed: {args.error or 'unknown'}"

    if args.duration:
        msg += f"\nDuration: {args.duration}"

    send_via_openclaw(msg, args.target)


if __name__ == "__main__":
    main()
