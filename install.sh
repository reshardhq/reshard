#!/bin/sh

set -eu

REBEAM_REPO="${REBEAM_REPO:-T31K/rebeam}"
REBEAM_VERSION="${REBEAM_VERSION:-latest}"
REBEAM_INSTALL_DIR="${REBEAM_INSTALL_DIR:-${HOME}/.local/bin}"

case "$(uname -s)" in
  Darwin) rebeam_os="apple-darwin" ;;
  Linux) rebeam_os="unknown-linux-gnu" ;;
  *) echo "rebeam: unsupported operating system: $(uname -s)" >&2; exit 1 ;;
esac

case "$(uname -m)" in
  arm64|aarch64) rebeam_arch="aarch64" ;;
  x86_64|amd64) rebeam_arch="x86_64" ;;
  *) echo "rebeam: unsupported CPU architecture: $(uname -m)" >&2; exit 1 ;;
esac

rebeam_artifact="rebeam-${rebeam_arch}-${rebeam_os}.tar.gz"
if [ -n "${REBEAM_DOWNLOAD_BASE:-}" ]; then
  rebeam_base="${REBEAM_DOWNLOAD_BASE%/}"
elif [ "$REBEAM_VERSION" = "latest" ]; then
  rebeam_base="https://github.com/${REBEAM_REPO}/releases/latest/download"
else
  rebeam_base="https://github.com/${REBEAM_REPO}/releases/download/${REBEAM_VERSION}"
fi

rebeam_tmp="$(mktemp -d 2>/dev/null || mktemp -d -t rebeam)"
trap 'rm -rf "$rebeam_tmp"' EXIT HUP INT TERM

rebeam_download() {
  rebeam_url="$1"
  rebeam_output="$2"
  if command -v curl >/dev/null 2>&1; then
    curl -fsSL "$rebeam_url" -o "$rebeam_output"
  elif command -v wget >/dev/null 2>&1; then
    wget -q "$rebeam_url" -O "$rebeam_output"
  else
    echo "rebeam: curl or wget is required" >&2
    exit 1
  fi
}

echo "Downloading ${rebeam_artifact}..."
rebeam_download "${rebeam_base}/${rebeam_artifact}" "${rebeam_tmp}/${rebeam_artifact}"
rebeam_download "${rebeam_base}/checksums.txt" "${rebeam_tmp}/checksums.txt"

rebeam_expected="$(awk -v name="$rebeam_artifact" '$2 == name || $2 == "*" name { print $1; exit }' "${rebeam_tmp}/checksums.txt")"
if [ -z "$rebeam_expected" ]; then
  echo "rebeam: ${rebeam_artifact} is missing from checksums.txt" >&2
  exit 1
fi

if command -v sha256sum >/dev/null 2>&1; then
  rebeam_actual="$(sha256sum "${rebeam_tmp}/${rebeam_artifact}" | awk '{print $1}')"
elif command -v shasum >/dev/null 2>&1; then
  rebeam_actual="$(shasum -a 256 "${rebeam_tmp}/${rebeam_artifact}" | awk '{print $1}')"
else
  echo "rebeam: sha256sum or shasum is required to verify the download" >&2
  exit 1
fi

if [ "$rebeam_actual" != "$rebeam_expected" ]; then
  echo "rebeam: checksum verification failed" >&2
  exit 1
fi

tar -xzf "${rebeam_tmp}/${rebeam_artifact}" -C "$rebeam_tmp"
if [ ! -f "${rebeam_tmp}/rebeam" ]; then
  echo "rebeam: release archive does not contain the rebeam binary" >&2
  exit 1
fi

mkdir -p "$REBEAM_INSTALL_DIR"
cp "${rebeam_tmp}/rebeam" "${REBEAM_INSTALL_DIR}/rebeam"
chmod 755 "${REBEAM_INSTALL_DIR}/rebeam"

echo "Installed rebeam to ${REBEAM_INSTALL_DIR}/rebeam"
case ":${PATH}:" in
  *":${REBEAM_INSTALL_DIR}:"*) ;;
  *) echo "Add ${REBEAM_INSTALL_DIR} to PATH to run rebeam from any shell." ;;
esac
