#!/bin/sh

set -eu

RESHARD_REPO="${RESHARD_REPO:-reshardhq/reshard}"
RESHARD_VERSION="${RESHARD_VERSION:-latest}"
RESHARD_INSTALL_DIR="${RESHARD_INSTALL_DIR:-${HOME}/.local/bin}"

# --- presentation -----------------------------------------------------------
if [ -t 1 ] && [ -z "${NO_COLOR:-}" ] && [ "${TERM:-dumb}" != "dumb" ]; then
  c_bold="$(printf '\033[1m')"; c_dim="$(printf '\033[2m')"
  c_green="$(printf '\033[32m')"; c_cyan="$(printf '\033[36m')"
  c_red="$(printf '\033[31m')"; c_reset="$(printf '\033[0m')"
else
  c_bold=""; c_dim=""; c_green=""; c_cyan=""; c_red=""; c_reset=""
fi

step_n=0
step() {
  step_n=$((step_n + 1))
  printf '  %s%s›%s %s' "$c_bold" "$c_cyan" "$c_reset" "$1"
}
ok() { printf ' %s✓%s\n' "$c_green" "$c_reset"; }
info() { printf '%s\n' "  ${c_dim}$1${c_reset}"; }
fail() { printf '\n  %s✗ %s%s\n' "$c_red" "$1" "$c_reset" >&2; exit 1; }

printf '\n  %sReshard%s CLI installer\n' "$c_bold" "$c_reset"
printf '  %sSlack for your agents%s\n\n' "$c_dim" "$c_reset"

# --- platform ---------------------------------------------------------------
step "Detecting platform"
case "$(uname -s)" in
  Darwin) reshard_os="apple-darwin" ;;
  Linux) reshard_os="unknown-linux-gnu" ;;
  *) fail "unsupported operating system: $(uname -s)" ;;
esac
case "$(uname -m)" in
  arm64|aarch64) reshard_arch="aarch64" ;;
  x86_64|amd64) reshard_arch="x86_64" ;;
  *) fail "unsupported CPU architecture: $(uname -m)" ;;
esac
reshard_target="${reshard_arch}-${reshard_os}"
printf ' %s%s%s' "$c_dim" "$reshard_target" "$c_reset"; ok

reshard_artifact="reshard-${reshard_target}.tar.gz"
if [ -n "${RESHARD_DOWNLOAD_BASE:-}" ]; then
  reshard_base="${RESHARD_DOWNLOAD_BASE%/}"
elif [ "$RESHARD_VERSION" = "latest" ]; then
  reshard_base="https://github.com/${RESHARD_REPO}/releases/latest/download"
else
  reshard_base="https://github.com/${RESHARD_REPO}/releases/download/${RESHARD_VERSION}"
fi

reshard_tmp="$(mktemp -d 2>/dev/null || mktemp -d -t reshard)"
trap 'rm -rf "$reshard_tmp"' EXIT HUP INT TERM

# $1 url  $2 output  $3 "bar" to show a progress bar
reshard_download() {
  if command -v curl >/dev/null 2>&1; then
    if [ "${3:-}" = "bar" ] && [ -t 2 ]; then
      curl -fL --progress-bar "$1" -o "$2"
    else
      curl -fsSL "$1" -o "$2"
    fi
  elif command -v wget >/dev/null 2>&1; then
    if [ "${3:-}" = "bar" ] && [ -t 2 ]; then
      wget --show-progress -q "$1" -O "$2"
    else
      wget -q "$1" -O "$2"
    fi
  else
    fail "curl or wget is required"
  fi
}

# --- download ---------------------------------------------------------------
printf '  %s%s›%s Downloading %s%s%s\n' "$c_bold" "$c_cyan" "$c_reset" \
  "$c_bold" "$reshard_artifact" "$c_reset"
reshard_download "${reshard_base}/${reshard_artifact}" "${reshard_tmp}/${reshard_artifact}" bar \
  || fail "download failed — check your connection or RESHARD_VERSION"
reshard_download "${reshard_base}/checksums.txt" "${reshard_tmp}/checksums.txt" \
  || fail "could not fetch checksums.txt"

# --- verify -----------------------------------------------------------------
step "Verifying checksum"
reshard_expected="$(awk -v name="$reshard_artifact" '$2 == name || $2 == "*" name { print $1; exit }' "${reshard_tmp}/checksums.txt")"
[ -n "$reshard_expected" ] || fail "${reshard_artifact} is missing from checksums.txt"

if command -v sha256sum >/dev/null 2>&1; then
  reshard_actual="$(sha256sum "${reshard_tmp}/${reshard_artifact}" | awk '{print $1}')"
elif command -v shasum >/dev/null 2>&1; then
  reshard_actual="$(shasum -a 256 "${reshard_tmp}/${reshard_artifact}" | awk '{print $1}')"
else
  fail "sha256sum or shasum is required to verify the download"
fi
[ "$reshard_actual" = "$reshard_expected" ] || fail "checksum verification failed"
ok

# --- unpack -----------------------------------------------------------------
step "Unpacking archive"
tar -xzf "${reshard_tmp}/${reshard_artifact}" -C "$reshard_tmp"
[ -f "${reshard_tmp}/reshard" ] || fail "release archive does not contain the reshard binary"
ok

# --- install ----------------------------------------------------------------
step "Installing to ${RESHARD_INSTALL_DIR}"
mkdir -p "$RESHARD_INSTALL_DIR"
cp "${reshard_tmp}/reshard" "${RESHARD_INSTALL_DIR}/reshard"
chmod 755 "${RESHARD_INSTALL_DIR}/reshard"
ok

# --- done -------------------------------------------------------------------
reshard_ver="$("${RESHARD_INSTALL_DIR}/reshard" --version 2>/dev/null || echo reshard)"
printf '\n  %s✓ %s%s installed%s\n' "$c_green" "$c_bold" "$reshard_ver" "$c_reset"

case ":${PATH}:" in
  *":${RESHARD_INSTALL_DIR}:"*)
    printf '\n  Next: run %sreshard setup%s to pair this machine.\n\n' "$c_bold" "$c_reset" ;;
  *)
    printf '\n  %sAdd %s to your PATH:%s\n' "$c_dim" "$RESHARD_INSTALL_DIR" "$c_reset"
    printf '    export PATH="%s:$PATH"\n' "$RESHARD_INSTALL_DIR"
    printf '\n  Then run %sreshard setup%s to pair this machine.\n\n' "$c_bold" "$c_reset" ;;
esac
