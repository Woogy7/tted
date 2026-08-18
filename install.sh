#!/bin/sh
set -eu

repository="Woogy7/tted"
version="${TTED_VERSION:-latest}"
install_dir="${TTED_INSTALL_DIR:-${HOME}/.local/bin}"

case "$(uname -s)-$(uname -m)" in
  Linux-x86_64) target="x86_64-unknown-linux-gnu" ;;
  Linux-aarch64|Linux-arm64) target="aarch64-unknown-linux-gnu" ;;
  Darwin-x86_64) target="x86_64-apple-darwin" ;;
  Darwin-arm64) target="aarch64-apple-darwin" ;;
  *) echo "Unsupported platform: $(uname -s) $(uname -m)" >&2; exit 1 ;;
esac

if [ "$version" = latest ]; then
  url="https://github.com/${repository}/releases/latest/download/tted-${target}.tar.gz"
else
  url="https://github.com/${repository}/releases/download/${version}/tted-${target}.tar.gz"
fi

temporary="$(mktemp -d)"
trap 'rm -rf "$temporary"' EXIT HUP INT TERM
curl --fail --location "$url" --output "$temporary/tted.tar.gz"
tar -xzf "$temporary/tted.tar.gz" -C "$temporary"
mkdir -p "$install_dir"
install -m 0755 "$temporary/tted" "$install_dir/tted"
echo "Installed TTED to $install_dir/tted"
