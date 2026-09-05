#!/bin/sh
set -e

REPO="${RUSH_REPO:-FolderFile/rush}"
DEST="${RUSH_DEST:-/usr/bin/rush}"
case "$(uname -m)" in
    x86_64|amd64) ASSET="rush-linux-x86_64" ;;
    aarch64|arm64) ASSET="rush-linux-aarch64" ;;
    *)
        echo "rush: unsupported architecture: $(uname -m)" >&2
        exit 1
        ;;
esac

TMP="$(mktemp)"
SUMS="$(mktemp)"
trap 'rm -f "$TMP" "$SUMS"' EXIT

fetch() {
    asset="$1"
    dest="$2"
    url="https://github.com/$REPO/releases/latest/download/$asset"
    if command -v curl >/dev/null 2>&1; then
        curl -fsSL "$url" -o "$dest" && return 0
    elif command -v wget >/dev/null 2>&1; then
        wget -qO "$dest" "$url" && return 0
    fi
    rm -f "$dest"
    if command -v gh >/dev/null 2>&1; then
        gh release download --repo "$REPO" --pattern "$asset" --output "$dest" && return 0
    fi
    if [ -n "$GITHUB_TOKEN" ]; then
        curl -fsSL -H "Authorization: token $GITHUB_TOKEN" "$url" -o "$dest" && return 0
    fi
    return 1
}

if ! fetch "$ASSET" "$TMP"; then
    echo "rush: download failed (private repo? run 'gh auth login' or set GITHUB_TOKEN)" >&2
    exit 1
fi

if fetch "SHA256SUMS" "$SUMS" && command -v sha256sum >/dev/null 2>&1; then
    grep " $ASSET\$" "$SUMS" | sed "s#  $ASSET#  $TMP#" | sha256sum -c - >/dev/null 2>&1 \
        || { echo "rush: checksum mismatch, aborting" >&2; exit 1; }
else
    echo "rush: warning: could not verify checksum" >&2
fi

if [ "$(id -u)" -ne 0 ]; then
    if command -v sudo >/dev/null 2>&1; then
        sudo install -m 755 "$TMP" "$DEST"
    else
        echo "rush: install needs root (run with sudo)" >&2
        exit 1
    fi
else
    install -m 755 "$TMP" "$DEST"
fi

echo "rush installed to $DEST:"
"$DEST" --version

cat <<'HINT'

to run a server:   rush -s -p 8080        (or install a service: rush -si -p 8080)
to connect:        rush host -p 8080
to update later:   rush --update
to remove:         rush --uninstall
HINT
