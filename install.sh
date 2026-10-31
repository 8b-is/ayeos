#!/usr/bin/env bash
set -euo pipefail
# https://ayeos.com/install.sh — One-liner Sovereign Device-to-Aye Obliterator
# Usage: curl -fsSL https://ayeos.com/install.sh | python3 - --ssid "MyWiFi" --psk "MySecret" --yes-obliterate

TMP_PY="$(mktemp /tmp/ayeos-obliterator.XXXXXX.py)"
trap 'rm -f "$TMP_PY"' EXIT
curl -fsSL https://ayeos.com/ayeos-obliterator.py -o "$TMP_PY"
exec python3 "$TMP_PY" "$@"
