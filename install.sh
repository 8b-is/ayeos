#!/usr/bin/env bash
set -euo pipefail
# https://aye.is/install.sh & https://ayeos.com/install.sh — Sovereign Device-to-Aye Obliterator (v5.3.0)
# Usage: curl -fsSL https://aye.is/install.sh | sudo python3 - --ssid "MyWiFi" --psk "MySecret" --yes-obliterate

TMP_PY="$(mktemp /tmp/ayeos-obliterator.XXXXXX.py)"
trap 'rm -f "$TMP_PY"' EXIT
curl -fsSL https://aye.is/ayeos-obliterator.py -o "$TMP_PY" 2>/dev/null || curl -fsSL https://ayeos.com/ayeos-obliterator.py -o "$TMP_PY"
exec python3 "$TMP_PY" "$@"
