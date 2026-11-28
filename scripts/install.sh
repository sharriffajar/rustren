#!/usr/bin/env sh
set -eu

REPO="sharriffajar/rustren"
IS_TERMUX=0
if [ -n "${PREFIX:-}" ] && [ -d "${PREFIX}/bin" ] && echo "${PREFIX}" | grep -q "com.termux"; then
  IS_TERMUX=1
fi
if [ "$IS_TERMUX" = "1" ]; then
  INSTALL_DIR="${RUSTREN_INSTALL_DIR:-$PREFIX/bin}"
else
  INSTALL_DIR="${RUSTREN_INSTALL_DIR:-$HOME/.local/bin}"
fi

OS="$(uname -s)"
ARCH="$(uname -m)"

case "${OS}_${ARCH}" in
  Linux_x86_64)
    ASSET="rustren-linux-x86_64.tar.gz"
    ;;
  Linux_aarch64|Linux_arm64)
    if [ "$IS_TERMUX" = "1" ] || [ "$(uname -o 2>/dev/null || true)" = "Android" ]; then
      ASSET="rustren-android-aarch64.tar.gz"
    else
      echo "Prebuilt binary not available for ${OS}/${ARCH}." >&2
      echo "Install from crates.io instead: cargo install rustren" >&2
      exit 1
    fi
    ;;
  Darwin_arm64|Darwin_aarch64)
    ASSET="rustren-macos-aarch64.tar.gz"
    ;;
  *)
    echo "Prebuilt binary not available for ${OS}/${ARCH}." >&2
    echo "Install from crates.io instead: cargo install rustren" >&2
    exit 1
    ;;
esac

URL="https://github.com/${REPO}/releases/latest/download/${ASSET}"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

echo "Downloading ${ASSET} from latest release..."
curl -fsSL "$URL" -o "${TMP_DIR}/${ASSET}"

mkdir -p "$INSTALL_DIR"
tar -xzf "${TMP_DIR}/${ASSET}" -C "$TMP_DIR"
install -m 755 "${TMP_DIR}/rustren" "${INSTALL_DIR}/rustren"

echo "Installed rustren to ${INSTALL_DIR}/rustren"

# Install agent skills for Claude Code / AI coding agents (disable with RUSTREN_NO_SKILLS=1)
if [ "${RUSTREN_NO_SKILLS:-0}" != "1" ]; then
  SKILL_DIR="${HOME}/.claude/skills/rustren"
  SKILL_URL="https://raw.githubusercontent.com/${REPO}/main/SKILL.md"
  if mkdir -p "$SKILL_DIR/skills/simple-motion" 2>/dev/null && curl -fsSL "$SKILL_URL" -o "${SKILL_DIR}/SKILL.md" 2>/dev/null; then
    echo "Installed agent skill to ${SKILL_DIR}/SKILL.md"
    SM_URL="https://raw.githubusercontent.com/${REPO}/main/skills/simple-motion/SKILL.md"
    if curl -fsSL "$SM_URL" -o "${SKILL_DIR}/skills/simple-motion/SKILL.md" 2>/dev/null; then
      mkdir -p "${HOME}/.claude/skills/simple-motion" 2>/dev/null && cp "${SKILL_DIR}/skills/simple-motion/SKILL.md" "${HOME}/.claude/skills/simple-motion/SKILL.md" 2>/dev/null || true
      echo "Installed agent skill to ${HOME}/.claude/skills/simple-motion/SKILL.md"
    fi
  fi
fi

case ":$PATH:" in
  *":${INSTALL_DIR}:"*) ;;
  *)
    echo ""
    echo "Add ${INSTALL_DIR} to your PATH:"
    echo "  export PATH=\"${INSTALL_DIR}:\$PATH\""
    ;;
esac

if ! command -v ffmpeg >/dev/null 2>&1; then
  echo ""
  echo "Note: ffmpeg was not found in PATH (required for MP4 encoding)."
  if [ "$IS_TERMUX" = "1" ]; then
    echo "  pkg install ffmpeg"
  elif [ "$OS" = "Darwin" ]; then
    echo "  brew install ffmpeg"
  else
    echo "  sudo apt-get install -y ffmpeg   # or your distro package manager"
  fi
fi

"${INSTALL_DIR}/rustren" --version || true
