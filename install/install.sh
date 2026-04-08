#!/usr/bin/env bash
set -euo pipefail

REPO="mateoltd/full-kvm"
INSTALL_DIR="${HOME}/.full-kvm/bin"

main() {
  echo "full-kvm installer"
  echo ""

  # detect platform
  OS="$(uname -s)"
  ARCH="$(uname -m)"

  case "${OS}" in
    Darwin) PLATFORM="apple-darwin" ;;
    Linux)  PLATFORM="unknown-linux-gnu" ;;
    *)      echo "unsupported OS: ${OS}"; exit 1 ;;
  esac

  case "${ARCH}" in
    x86_64)  TARGET="${ARCH}-${PLATFORM}" ;;
    aarch64|arm64) TARGET="aarch64-${PLATFORM}" ;;
    *)       echo "unsupported architecture: ${ARCH}"; exit 1 ;;
  esac

  echo "detected: ${TARGET}"

  # get latest release
  LATEST=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" | grep '"tag_name"' | sed -E 's/.*"([^"]+)".*/\1/')
  if [ -z "${LATEST}" ]; then
    echo "could not determine latest version"
    exit 1
  fi
  echo "latest version: ${LATEST}"

  # check if already installed at this version
  if [ -f "${INSTALL_DIR}/full-kvm" ]; then
    CURRENT=$("${INSTALL_DIR}/full-kvm" --version 2>/dev/null || echo "unknown")
    if echo "${CURRENT}" | grep -q "${LATEST#v}"; then
      echo "already up to date (${LATEST})"
      run_setup
      exit 0
    fi
    echo "updating from ${CURRENT} to ${LATEST}"
  fi

  # download and extract
  DOWNLOAD_URL="https://github.com/${REPO}/releases/download/${LATEST}/full-kvm-${LATEST}-${TARGET}.tar.gz"
  echo "downloading ${DOWNLOAD_URL}"

  mkdir -p "${INSTALL_DIR}"
  curl -fsSL "${DOWNLOAD_URL}" | tar xz -C "${INSTALL_DIR}"

  # add to PATH
  add_to_path

  echo "installed to ${INSTALL_DIR}"
  run_setup
}

add_to_path() {
  local shell_profile=""
  case "${SHELL}" in
    */zsh)  shell_profile="${HOME}/.zshrc" ;;
    */bash) shell_profile="${HOME}/.bashrc" ;;
    *)      shell_profile="${HOME}/.profile" ;;
  esac

  if [ -f "${shell_profile}" ]; then
    if ! grep -q "full-kvm/bin" "${shell_profile}" 2>/dev/null; then
      echo "" >> "${shell_profile}"
      echo "# full-kvm" >> "${shell_profile}"
      echo "export PATH=\"\${HOME}/.full-kvm/bin:\${PATH}\"" >> "${shell_profile}"
      echo "added ${INSTALL_DIR} to PATH in ${shell_profile}"
    fi
  fi

  export PATH="${INSTALL_DIR}:${PATH}"
}

run_setup() {
  if [ -f "${INSTALL_DIR}/full-kvm-setup" ]; then
    echo ""
    "${INSTALL_DIR}/full-kvm-setup"
  fi
}

main "$@"
