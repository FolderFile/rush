#!/bin/sh
set -e

REPO="${RUSH_REPO:-FolderFile/rush}"
DEST="${RUSH_DEST:-/usr/bin/rush}"

case "$(uname -m)" in
    x86_64|amd64) ASSET="rush-linux" ;;
    *)
        echo "rush: unsupported architecture: $(uname -m)" >&2
        exit 1
        ;;
esac

if [ "$(id -u)" -ne 0 ]; then
    if command -v sudo >/dev/null 2>&1; then
        exec sudo "$0" "$@"
    fi
    echo "rush: install needs root (run with sudo)" >&2
    exit 1
fi

URL="https://github.com/$REPO/releases/latest/download/$ASSET"
TMP="$(mktemp)"
trap 'rm -f "$TMP"' EXIT

if command -v curl >/dev/null 2>&1; then
    curl -fsSL "$URL" -o "$TMP" || true
elif command -v wget >/dev/null 2>&1; then
    wget -qO "$TMP" "$URL" || true
fi

if [ ! -s "$TMP" ]; then
    rm -f "$TMP"
    if command -v gh >/dev/null 2>&1; then
        gh release download --repo "$REPO" --pattern "$ASSET" --output "$TMP"
    elif [ -n "$GITHUB_TOKEN" ]; then
        curl -fsSL -H "Authorization: token $GITHUB_TOKEN" "$URL" -o "$TMP"
    else
        echo "rush: download failed (private repo? run 'gh auth login' or set GITHUB_TOKEN)" >&2
        exit 1
    fi
fi

chmod 755 "$TMP"
install -m 755 "$TMP" "$DEST"

echo "rush installed to $DEST:"
"$DEST" --version

cat <<'HINT'

to run a server:   rush -s -p 8080        (or install a service: rush -si -p 8080)
to connect:        rush host -p 8080
to update later:   rush --update
to remove:         rush --uninstall
HINT
