#!/bin/sh
set -eu

repository="Woogy7/tted"
version="${TTED_VERSION:-latest}"
install_dir="${TTED_INSTALL_DIR:-}"
force_source=0

usage() {
    cat <<'EOF'
Install TTED

Usage: install.sh [--version v0.1.0] [--install-dir DIR] [--source]

Environment variables:
  TTED_VERSION       Release tag to install (default: latest)
  TTED_INSTALL_DIR   Destination directory (default: ~/.local/bin)
EOF
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --version) version="${2:?--version needs a tag}"; shift 2 ;;
        --install-dir) install_dir="${2:?--install-dir needs a directory}"; shift 2 ;;
        --source) force_source=1; shift ;;
        -h|--help) usage; exit 0 ;;
        *) echo "Unknown option: $1" >&2; usage >&2; exit 2 ;;
    esac
done

if [ -z "$install_dir" ]; then
    if [ "$(id -u)" -eq 0 ]; then
        install_dir="/usr/local/bin"
    else
        install_dir="${HOME}/.local/bin"
    fi
fi

download() {
    source_url="$1"
    destination="$2"
    if command -v curl >/dev/null 2>&1; then
        curl --proto '=https' --tlsv1.2 --fail --location --silent --show-error \
            "$source_url" --output "$destination"
    elif command -v wget >/dev/null 2>&1; then
        wget --https-only --quiet "$source_url" --output-document="$destination"
    else
        echo "TTED installation needs curl or wget." >&2
        return 1
    fi
}

platform_target() {
    case "$(uname -s)-$(uname -m)" in
        Linux-x86_64) echo "x86_64-unknown-linux-gnu" ;;
        Linux-aarch64|Linux-arm64) echo "aarch64-unknown-linux-gnu" ;;
        Darwin-x86_64) echo "x86_64-apple-darwin" ;;
        Darwin-arm64) echo "aarch64-apple-darwin" ;;
        *) return 1 ;;
    esac
}

verify_checksum() {
    archive="$1"
    checksum_file="$2"
    if command -v sha256sum >/dev/null 2>&1; then
        (cd "$(dirname "$archive")" && sha256sum -c "$(basename "$checksum_file")")
    elif command -v shasum >/dev/null 2>&1; then
        (cd "$(dirname "$archive")" && shasum -a 256 -c "$(basename "$checksum_file")")
    else
        echo "Cannot verify the TTED download: sha256sum or shasum is required." >&2
        return 1
    fi
}

install_release() {
    target="$(platform_target)" || return 1
    temporary="$(mktemp -d)"
    archive="$temporary/tted-${target}.tar.gz"
    checksum="$archive.sha256"
    if [ "$version" = latest ]; then
        base_url="https://github.com/${repository}/releases/latest/download"
    else
        base_url="https://github.com/${repository}/releases/download/${version}"
    fi
    if ! download "$base_url/tted-${target}.tar.gz" "$archive"; then
        rm -rf "$temporary"
        return 1
    fi
    if ! download "$base_url/tted-${target}.tar.gz.sha256" "$checksum"; then
        echo "Release checksum is unavailable; refusing an unverified install." >&2
        rm -rf "$temporary"
        return 1
    fi
    if ! verify_checksum "$archive" "$checksum"; then
        echo "TTED release checksum verification failed." >&2
        rm -rf "$temporary"
        return 1
    fi
    if ! tar -xzf "$archive" -C "$temporary"; then
        rm -rf "$temporary"
        return 1
    fi
    mkdir -p "$install_dir"
    if ! install -m 0755 "$temporary/tted" "$install_dir/tted"; then
        rm -rf "$temporary"
        return 1
    fi
    rm -rf "$temporary"
}

install_source() {
    if ! command -v cargo >/dev/null 2>&1; then
        cat >&2 <<'EOF'
No prebuilt TTED release is available for this platform, and Cargo was not found.
Install Rust from https://rustup.rs/ and run this installer again, or follow the
distribution-specific source instructions in TTED's README.
EOF
        exit 1
    fi
    case "$install_dir" in
        */bin) install_root=${install_dir%/bin} ;;
        *)
            echo "Source fallback requires an install directory ending in /bin." >&2
            exit 1
            ;;
    esac
    set -- install --locked --force --git "https://github.com/${repository}.git" --root "$install_root"
    if [ "$version" != latest ]; then
        set -- "$@" --tag "$version"
    fi
    cargo "$@" tted
}

if [ "$force_source" -eq 1 ] || ! install_release; then
    echo "No compatible verified release found; building TTED from source."
    install_source
fi

case ":${PATH}:" in
    *:${install_dir}:*) ;;
    *)
        if [ "$install_dir" = "${HOME}/.local/bin" ]; then
            shell_name=$(basename "${SHELL:-sh}")
            case "$shell_name" in
                zsh) profile="${HOME}/.zshrc" ;;
                fish)
                    echo "Add ${install_dir} to fish_user_paths, then open a new terminal." >&2
                    profile=""
                    ;;
                *) profile="${HOME}/.profile" ;;
            esac
            if [ -n "$profile" ] && ! grep -F 'export PATH="$HOME/.local/bin:$PATH"' "$profile" >/dev/null 2>&1; then
                printf '\n# TTED\nexport PATH="$HOME/.local/bin:$PATH"\n' >> "$profile"
                echo "Added ~/.local/bin to PATH in $profile (applies to new terminals)."
            fi
        else
            echo "Add $install_dir to PATH to run tted by name." >&2
        fi
        ;;
esac

echo "Installed TTED to $install_dir/tted"
echo "Open a new terminal, then run: tted ."
