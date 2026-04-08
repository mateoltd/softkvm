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

  install_m1ddc
  install_deskflow
  register_path
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

  local build_log="${build_dir}/build.log"
  info "building (release mode)"
  if ! cargo build --release --manifest-path "${build_dir}/Cargo.toml" \
    --workspace \
    --features softkvm-orchestrator/real-ddc,softkvm-cli/real-ddc >"${build_log}" 2>&1; then
    echo ""
    tail -10 "${build_log}"
    echo ""
    error "build failed"
    return 1
  fi

  info "copying binaries"
  local missing=0
  for bin in softkvm softkvm-orchestrator softkvm-agent; do
    if [ -f "${build_dir}/target/release/${bin}" ]; then
      cp "${build_dir}/target/release/${bin}" "${INSTALL_DIR}/"
      chmod +x "${INSTALL_DIR}/${bin}"
    else
      error "binary not found: ${bin}"
      missing=$((missing + 1))
    fi
  done
  if [ "${missing}" -gt 0 ]; then
    error "build incomplete: ${missing} binary(ies) missing"
    return 1
  fi

  # bundle and install the setup TUI while we still have the source tree
  local setup_dir="${build_dir}/setup"
  if [ -f "${setup_dir}/package.json" ]; then
    local bundler=""
    if command -v bun &>/dev/null; then
      bundler="bun"
    elif command -v npx &>/dev/null; then
      bundler="npx"
    fi
    if [ -n "${bundler}" ]; then
      info "bundling setup wizard"
      local setup_dest="${INSTALL_DIR}/../setup"
      mkdir -p "${setup_dest}"
      if [ "${bundler}" = "bun" ]; then
        (cd "${setup_dir}" && bun install --silent 2>/dev/null && bun build --outfile="${setup_dest}/setup.mjs" --target=node src/index.ts 2>/dev/null) || true
      else
        (cd "${setup_dir}" && npm install --silent 2>/dev/null && npx esbuild --bundle --platform=node --format=esm --outfile="${setup_dest}/setup.mjs" src/index.ts 2>/dev/null) || true
      fi
      if [ -f "${setup_dest}/setup.mjs" ]; then
        cat > "${INSTALL_DIR}/softkvm-setup" << 'WRAPPER'
#!/usr/bin/env bash
SETUP_DIR="$(dirname "$(dirname "$(readlink -f "$0" 2>/dev/null || realpath "$0" 2>/dev/null || echo "$0")")")/setup"
exec node "${SETUP_DIR}/setup.mjs" "$@"
WRAPPER
        chmod +x "${INSTALL_DIR}/softkvm-setup"
      else
        warn "setup wizard build failed (will use manual setup)"
      fi
    fi
  fi

  return 0
}

install_m1ddc() {
  # m1ddc is Apple Silicon only, provides DDC/CI through paths that
  # ddc-hi/IOKit misses (USB-C adapters, etc.)
  if [ "${OS}" != "Darwin" ] || [ "${ARCH}" != "arm64" ]; then
    return 0
  fi

  # already installed?
  if [ -f "${INSTALL_DIR}/m1ddc" ]; then
    info "m1ddc already installed"
    return 0
  fi

  # need clang (ships with Xcode command line tools on every Mac)
  if ! command -v clang &>/dev/null; then
    warn "clang not found, skipping m1ddc (install Xcode command line tools)"
    return 0
  fi

  local m1ddc_dir
  m1ddc_dir="$(mktemp -d)"

  info "building m1ddc (Apple Silicon DDC backend)"
  if git clone --depth 1 https://github.com/waydabber/m1ddc.git "${m1ddc_dir}" 2>/dev/null; then
    if (cd "${m1ddc_dir}" && make binary 2>/dev/null); then
      if [ -f "${m1ddc_dir}/m1ddc" ]; then
        cp "${m1ddc_dir}/m1ddc" "${INSTALL_DIR}/"
        chmod +x "${INSTALL_DIR}/m1ddc"
        info "m1ddc installed"
      else
        warn "m1ddc build produced no binary"
      fi
    else
      warn "m1ddc build failed (DDC will use ddc-hi only)"
    fi
  else
    warn "could not clone m1ddc repository"
  fi

  rm -rf "${m1ddc_dir}"
  return 0
}

install_deskflow() {
  # check if deskflow-core is already available
  if command -v deskflow-core &>/dev/null; then
    info "deskflow-core already installed"
    return 0
  fi

  # check known install locations
  local known_paths=""
  case "${OS}" in
    Darwin)
      known_paths="/Applications/Deskflow.app/Contents/MacOS/deskflow-core"
      ;;
    Linux)
      known_paths="/usr/bin/deskflow-core /usr/local/bin/deskflow-core"
      ;;
  esac
  for p in ${known_paths}; do
    if [ -f "${p}" ]; then
      info "deskflow-core found at ${p}"
      return 0
    fi
  done

  info "installing deskflow (required for mouse/keyboard sharing)"

  case "${OS}" in
    Darwin)
      if command -v brew &>/dev/null; then
        if brew install --cask deskflow/tap/deskflow 2>/dev/null; then
          info "deskflow installed via homebrew"
          return 0
        fi
      fi
      warn "could not install deskflow automatically"
      echo "  install manually: brew install --cask deskflow/tap/deskflow"
      echo "  or download from: https://github.com/deskflow/deskflow/releases"
      ;;
    Linux)
      if command -v apt-get &>/dev/null; then
        if sudo apt-get install -y deskflow 2>/dev/null; then
          info "deskflow installed via apt"
          return 0
        fi
      elif command -v dnf &>/dev/null; then
        if sudo dnf install -y deskflow 2>/dev/null; then
          info "deskflow installed via dnf"
          return 0
        fi
      elif command -v pacman &>/dev/null; then
        if sudo pacman -S --noconfirm deskflow 2>/dev/null; then
          info "deskflow installed via pacman"
          return 0
        fi
      fi
      warn "could not install deskflow automatically"
      echo "  install via your package manager or download from:"
      echo "  https://github.com/deskflow/deskflow/releases"
      ;;
  esac

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

  # run interactive setup — use exec to hand full terminal control to
  # the TUI process (matches the pattern in working curl|sh installers)
  if [ -f "${INSTALL_DIR}/softkvm-setup" ]; then
    if [ -e /dev/tty ]; then
      exec "${INSTALL_DIR}/softkvm-setup" </dev/tty
    fi
    # /dev/tty unavailable or exec failed
    show_manual_setup
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
