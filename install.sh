#!/bin/sh
# ratasm installer.
#
#   curl -fsSL https://github.com/tuna4ll/ratasm/releases/latest/download/install.sh | sh
#
# Downloads the release binary matching this machine and installs it. Set
# RATASM_INSTALL_DIR to choose where it goes; the default is ~/.local/bin, which
# needs no privileges and is on PATH for most shells.
#
# Environment:
#   RATASM_VERSION      release tag to install (default: latest)
#   RATASM_INSTALL_DIR  directory to install into (default: ~/.local/bin)
#   RATASM_BASE_URL     download from somewhere else (a mirror, or a local
#                       file:// URL when testing this script)

set -eu

REPO="tuna4ll/ratasm"
BIN="ratasm"
VERSION="${RATASM_VERSION:-latest}"
INSTALL_DIR="${RATASM_INSTALL_DIR:-$HOME/.local/bin}"

# Colours only when writing to a terminal, so piped output stays clean.
if [ -t 1 ] && [ -z "${NO_COLOR:-}" ]; then
    BOLD=$(printf '\033[1m'); RED=$(printf '\033[31m')
    GREEN=$(printf '\033[32m'); YELLOW=$(printf '\033[33m')
    RESET=$(printf '\033[0m')
else
    BOLD=''; RED=''; GREEN=''; YELLOW=''; RESET=''
fi

say()  { printf '%s\n' "$*"; }
warn() { printf '%s%s%s\n' "$YELLOW" "$*" "$RESET" >&2; }
die()  { printf '%serror:%s %s\n' "$RED" "$RESET" "$*" >&2; exit 1; }

need() {
    command -v "$1" >/dev/null 2>&1 || die "'$1' is required but not installed."
}

# --- work out what to download ----------------------------------------------

detect_target() {
    os=$(uname -s)
    arch=$(uname -m)

    case "$os" in
        Linux) ;;
        Darwin) die "macOS is not supported: ratasm targets Linux ELF binaries." ;;
        *) die "unsupported operating system: $os" ;;
    esac

    case "$arch" in
        x86_64 | amd64) ;;
        *) die "unsupported architecture: $arch (ratasm supports x86_64 only)" ;;
    esac

    # Prefer the musl build: it runs on any glibc version, including ones
    # older than the machine the release was built on.
    printf 'x86_64-unknown-linux-musl'
}

download() {
    url="$1"
    output="$2"
    if command -v curl >/dev/null 2>&1; then
        curl -fsSL "$url" -o "$output"
    elif command -v wget >/dev/null 2>&1; then
        wget -qO "$output" "$url"
    else
        die "either curl or wget is required."
    fi
}

# --- install ----------------------------------------------------------------

main() {
    need uname
    need tar
    need install

    target=$(detect_target)

    if [ -n "${RATASM_BASE_URL:-}" ]; then
        base="$RATASM_BASE_URL"
    elif [ "$VERSION" = "latest" ]; then
        base="https://github.com/$REPO/releases/latest/download"
    else
        base="https://github.com/$REPO/releases/download/$VERSION"
    fi

    archive="$BIN-$target.tar.gz"
    say "${BOLD}Installing $BIN${RESET} ($target, $VERSION)"

    tmp=$(mktemp -d)
    # Clean up on every exit path, including interruption.
    trap 'rm -rf "$tmp"' EXIT INT TERM

    say "  downloading $archive"
    download "$base/$archive" "$tmp/$archive" \
        || die "download failed. Check that a release exists at https://github.com/$REPO/releases"

    # Verify the checksum when the release publishes one. A missing checksum
    # file is not fatal, but a mismatching checksum is.
    if download "$base/$archive.sha256" "$tmp/$archive.sha256" 2>/dev/null; then
        if command -v sha256sum >/dev/null 2>&1; then
            say "  verifying checksum"
            expected=$(cut -d' ' -f1 <"$tmp/$archive.sha256")
            actual=$(sha256sum "$tmp/$archive" | cut -d' ' -f1)
            [ "$expected" = "$actual" ] \
                || die "checksum mismatch: expected $expected, got $actual"
        else
            warn "  sha256sum not found; skipping checksum verification"
        fi
    else
        warn "  no checksum published for this release; skipping verification"
    fi

    say "  unpacking"
    tar -xzf "$tmp/$archive" -C "$tmp"
    [ -f "$tmp/$BIN" ] || die "the archive did not contain a '$BIN' binary."

    mkdir -p "$INSTALL_DIR"
    install -m 0755 "$tmp/$BIN" "$INSTALL_DIR/$BIN"
    say "  installed to $INSTALL_DIR/$BIN"

    # --- post-install advice -------------------------------------------------

    case ":$PATH:" in
        *":$INSTALL_DIR:"*) ;;
        *)
            warn ""
            warn "$INSTALL_DIR is not on your PATH. Add it with:"
            warn "  echo 'export PATH=\"\$PATH:$INSTALL_DIR\"' >> ~/.profile"
            warn "or, for fish:"
            warn "  fish_add_path $INSTALL_DIR"
            ;;
    esac

    missing=''
    for tool in nasm ld gdb; do
        command -v "$tool" >/dev/null 2>&1 || missing="$missing $tool"
    done
    if [ -n "$missing" ]; then
        warn ""
        warn "ratasm needs these tools, which are not installed:$missing"
        warn "  Debian/Ubuntu:  sudo apt install nasm binutils gdb"
        warn "  Fedora:         sudo dnf install nasm binutils gdb"
        warn "  Arch:           sudo pacman -S nasm binutils gdb"
    fi

    say ""
    say "${GREEN}Done.${RESET} Get started with:"
    say "  $BIN new hello"
    say "  cd hello && $BIN"
}

main "$@"
