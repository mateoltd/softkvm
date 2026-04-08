#!/usr/bin/env bash
set -euo pipefail

REPO="mateoltd/softkvm"
INSTALL_DIR="${HOME}/.softkvm/bin"
REPO_URL="https://github.com/${REPO}.git"

# colors (disabled if not a tty)
if [ -t 1 ]; then
  BOLD="\033[1m" DIM="\033[2m" GREEN="\033[32m"
  YELLOW="\033[33m" RED="\033[31m" RESET="\033[0m"
else
  BOLD="" DIM="" GREEN="" YELLOW="" RED="" RESET=""
fi

info()  { echo -e "${BOLD}${GREEN}▸${RESET} $*"; }
warn()  { echo -e "${BOLD}${YELLOW}▸${RESET} $*"; }
error() { echo -e "${BOLD}${RED}▸${RESET} $*"; }

main() {
  echo ""
  echo -e "${BOLD}softkvm installer${RESET}"
  echo ""

  detect_platform
  mkdir -p "${INSTALL_DIR}"

  if try_release_install; then
    info "installed from release"
  elif try_source_install; then
    info "built from source"
  else
    error "installation failed"
    echo ""
    echo "manual install: https://github.com/${REPO}#build-from-source"
    exit 1
  fi

  register_path
  build_setup_binary
  echo ""
  info "installed to ${INSTALL_DIR}"
  run_post_install
}

detect_platform() {
  OS="$(uname -s)"
  ARCH="$(uname -m)"

  case "${OS}" in
    Darwin) PLATFORM="apple-darwin" ;;
    Linux)  PLATFORM="unknown-linux-gnu" ;;
    *)      error "unsupported OS: ${OS}"; exit 1 ;;
  esac

  case "${ARCH}" in
    x86_64)       TARGET="${ARCH}-${PLATFORM}" ;;
    aarch64|arm64) TARGET="aarch64-${PLATFORM}" ;;
    *)            error "unsupported architecture: ${ARCH}"; exit 1 ;;
  esac

  info "platform: ${TARGET}"
}

try_release_install() {
  # check if curl and the GitHub API are available
  if ! command -v curl &>/dev/null; then
    return 1
  fi

  local latest
  latest=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" 2>/dev/null \
    | grep '"tag_name"' | sed -E 's/.*"([^"]+)".*/\1/') || true

  if [ -z "${latest:-}" ]; then
    warn "no releases found, falling back to source build"
    return 1
  fi

  # check if already at this version
  if [ -f "${INSTALL_DIR}/softkvm" ]; then
    local current
    current=$("${INSTALL_DIR}/softkvm" --version 2>/dev/null || echo "unknown")
    if echo "${current}" | grep -q "${latest#v}"; then
      info "already up to date (${latest})"
      return 0
    fi
  fi

  local url="https://github.com/${REPO}/releases/download/${latest}/softkvm-${latest}-${TARGET}.tar.gz"
  info "downloading ${latest} for ${TARGET}"

  if curl -fsSL "${url}" | tar xz -C "${INSTALL_DIR}" 2>/dev/null; then
    return 0
  else
    warn "release download failed, falling back to source build"
    return 1
  fi
}

try_source_install() {
  # need git + cargo
  if ! command -v git &>/dev/null; then
    error "git is required to build from source"
    return 1
  fi
  if ! command -v cargo &>/dev/null; then
    warn "rust not found, installing via rustup"
    if ! install_rust; then
      error "failed to install rust"
      echo "  install manually: https://rustup.rs"
      return 1
    fi
  fi

  local build_dir
  build_dir="$(mktemp -d)"
  trap "rm -rf '${build_dir}'" EXIT

  info "cloning repository"
  git clone --depth 1 "${REPO_URL}" "${build_dir}" 2>/dev/null

  info "building (release mode)"
  cargo build --release --manifest-path "${build_dir}/Cargo.toml" \
    --features real-ddc --no-default-features 2>&1 | tail -1

  info "copying binaries"
  for bin in softkvm softkvm-orchestrator softkvm-agent; do
    if [ -f "${build_dir}/target/release/${bin}" ]; then
      cp "${build_dir}/target/release/${bin}" "${INSTALL_DIR}/"
      chmod +x "${INSTALL_DIR}/${bin}"
    fi
  done

  return 0
}

install_rust() {
  if ! command -v curl &>/dev/null; then
    return 1
  fi
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --quiet 2>/dev/null
  # shellcheck disable=SC1091
  source "${HOME}/.cargo/env" 2>/dev/null || true
  command -v cargo &>/dev/null
}

build_setup_binary() {
  # build the setup TUI if bun is available
  if ! command -v bun &>/dev/null; then
    return 0
  fi

  local setup_dir
  # check if we cloned the repo during source build
  if [ -d "${INSTALL_DIR}/../setup" ]; then
    setup_dir="${INSTALL_DIR}/../setup"
  else
    # try to find it relative to the script
    local script_dir
    script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
    if [ -d "${script_dir}/../setup" ]; then
      setup_dir="${script_dir}/../setup"
    else
      return 0
    fi
  fi

  info "building setup wizard"
  (cd "${setup_dir}" && bun install --silent 2>/dev/null && bun run build 2>/dev/null) || true
  if [ -f "${setup_dir}/dist/softkvm-setup" ]; then
    cp "${setup_dir}/dist/softkvm-setup" "${INSTALL_DIR}/"
    chmod +x "${INSTALL_DIR}/softkvm-setup"
  fi
}

register_path() {
  # already in PATH?
  if echo "${PATH}" | grep -q "softkvm/bin"; then
    return 0
  fi

  local shell_profile=""
  case "${SHELL:-/bin/bash}" in
    */zsh)  shell_profile="${HOME}/.zshrc" ;;
    */bash) shell_profile="${HOME}/.bashrc" ;;
    */fish) shell_profile="${HOME}/.config/fish/config.fish" ;;
    *)      shell_profile="${HOME}/.profile" ;;
  esac

  if [ -n "${shell_profile}" ]; then
    if ! grep -q "softkvm/bin" "${shell_profile}" 2>/dev/null; then
      {
        echo ""
        echo "# softkvm"
        if [[ "${shell_profile}" == *"fish"* ]]; then
          echo "set -gx PATH \$HOME/.softkvm/bin \$PATH"
        else
          echo "export PATH=\"\${HOME}/.softkvm/bin:\${PATH}\""
        fi
      } >> "${shell_profile}"
      info "added to PATH in ${shell_profile}"
    fi
  fi

  export PATH="${INSTALL_DIR}:${PATH}"
}

run_post_install() {
  echo ""
  echo -e "${BOLD}scanning monitors${RESET}"
  echo ""

  # detect monitors
  if "${INSTALL_DIR}/softkvm" scan 2>/dev/null; then
    echo ""
  else
    warn "no DDC/CI monitors detected (can be configured manually)"
    echo ""
  fi

  # run interactive setup
  if [ -f "${INSTALL_DIR}/softkvm-setup" ]; then
    "${INSTALL_DIR}/softkvm-setup"
  elif command -v bun &>/dev/null; then
    local script_dir
    script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
    if [ -f "${script_dir}/../setup/src/index.ts" ]; then
      (cd "${script_dir}/../setup" && bun run dev)
    else
      show_manual_setup
    fi
  else
    show_manual_setup
  fi
}

show_manual_setup() {
  echo -e "${BOLD}next steps${RESET}"
  echo ""
  echo "  1. create a config file:"
  echo "     softkvm setup          (interactive, requires bun)"
  echo "     softkvm validate       (check an existing config)"
  echo ""
  echo "  2. start the daemon:"
  echo "     softkvm-orchestrator   (on the primary machine)"
  echo "     softkvm-agent          (on each secondary machine)"
  echo ""
  echo "  docs: https://github.com/${REPO}#quick-start"
}

main "$@"
