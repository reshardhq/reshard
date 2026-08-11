#!/bin/sh

set -eu

RESHARD_REPO="${RESHARD_REPO:-reshardhq/reshard}"
RESHARD_VERSION="${RESHARD_VERSION:-latest}"
RESHARD_INSTALL_DIR="${RESHARD_INSTALL_DIR:-${HOME}/.local/bin}"

case "$(uname -s)" in
  Darwin) reshard_os="apple-darwin" ;;
  Linux) reshard_os="unknown-linux-gnu" ;;
  *) echo "reshard: unsupported operating system: $(uname -s)" >&2; exit 1 ;;
esac

case "$(uname -m)" in
  arm64|aarch64) reshard_arch="aarch64" ;;
  x86_64|amd64) reshard_arch="x86_64" ;;
  *) echo "reshard: unsupported CPU architecture: $(uname -m)" >&2; exit 1 ;;
esac

reshard_artifact="reshard-${reshard_arch}-${reshard_os}.tar.gz"
if [ -n "${RESHARD_DOWNLOAD_BASE:-}" ]; then
  reshard_base="${RESHARD_DOWNLOAD_BASE%/}"
elif [ "$RESHARD_VERSION" = "latest" ]; then
  reshard_base="https://github.com/${RESHARD_REPO}/releases/latest/download"
else
  reshard_base="https://github.com/${RESHARD_REPO}/releases/download/${RESHARD_VERSION}"
fi

reshard_tmp="$(mktemp -d 2>/dev/null || mktemp -d -t reshard)"
trap 'rm -rf "$reshard_tmp"' EXIT HUP INT TERM

reshard_download() {
  reshard_url="$1"
  reshard_output="$2"
  if command -v curl >/dev/null 2>&1; then
    curl -fsSL "$reshard_url" -o "$reshard_output"
  elif command -v wget >/dev/null 2>&1; then
    wget -q "$reshard_url" -O "$reshard_output"
  else
    echo "reshard: curl or wget is required" >&2
    exit 1
  fi
}

echo "Downloading ${reshard_artifact}..."
reshard_download "${reshard_base}/${reshard_artifact}" "${reshard_tmp}/${reshard_artifact}"
reshard_download "${reshard_base}/checksums.txt" "${reshard_tmp}/checksums.txt"

reshard_expected="$(awk -v name="$reshard_artifact" '$2 == name || $2 == "*" name { print $1; exit }' "${reshard_tmp}/checksums.txt")"
if [ -z "$reshard_expected" ]; then
  echo "reshard: ${reshard_artifact} is missing from checksums.txt" >&2
  exit 1
fi

if command -v sha256sum >/dev/null 2>&1; then
  reshard_actual="$(sha256sum "${reshard_tmp}/${reshard_artifact}" | awk '{print $1}')"
elif command -v shasum >/dev/null 2>&1; then
  reshard_actual="$(shasum -a 256 "${reshard_tmp}/${reshard_artifact}" | awk '{print $1}')"
else
  echo "reshard: sha256sum or shasum is required to verify the download" >&2
  exit 1
fi

if [ "$reshard_actual" != "$reshard_expected" ]; then
  echo "reshard: checksum verification failed" >&2
  exit 1
fi

tar -xzf "${reshard_tmp}/${reshard_artifact}" -C "$reshard_tmp"
if [ ! -f "${reshard_tmp}/reshard" ]; then
  echo "reshard: release archive does not contain the reshard binary" >&2
  exit 1
fi

mkdir -p "$RESHARD_INSTALL_DIR"
cp "${reshard_tmp}/reshard" "${RESHARD_INSTALL_DIR}/reshard"
chmod 755 "${RESHARD_INSTALL_DIR}/reshard"

echo "Installed reshard to ${RESHARD_INSTALL_DIR}/reshard"
case ":${PATH}:" in
  *":${RESHARD_INSTALL_DIR}:"*) ;;
  *) echo "Add ${RESHARD_INSTALL_DIR} to PATH to run reshard from any shell." ;;
esac
