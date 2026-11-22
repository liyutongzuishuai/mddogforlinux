#!/bin/sh
# Installs the latest mdd release.
#   curl -fsSL https://raw.githubusercontent.com/liyutongzuishuai/mddogforlinux/main/install.sh | sh
# MDDOG_INSTALL_DIR picks the folder (default ~/.local/bin).
# MDDOG_RELEASE_URL overrides the platform's release download directory.
set -eu

dir=${MDDOG_INSTALL_DIR:-$HOME/.local/bin}
case $dir in /*) ;; *) dir="$(pwd -P)/$dir" ;; esac

# Color only on a terminal, and never with NO_COLOR set (no-color.org).
ok='✓' err='error:'
if [ -z "${NO_COLOR:-}" ]; then
  if [ -t 1 ]; then ok=$(printf '\033[32m✓\033[0m'); fi
  if [ -t 2 ]; then err=$(printf '\033[31merror:\033[0m'); fi
fi
fail() { echo "mddog install: $err $*" >&2; exit 1; }

# Show ~ for a folder under the user's home.
case $dir in
  "$HOME"/*) show="~${dir#"$HOME"}" ;;
  *) show=$dir ;;
esac

os=$(uname -s)
arch=$(uname -m)
case $os in
  Darwin)
    # Rosetta reports x86_64 on Apple silicon; the hardware still reports arm64.
    if command -v sysctl >/dev/null 2>&1 && [ "$(sysctl -n hw.optional.arm64 2>/dev/null || true)" = 1 ]; then arch=arm64; fi
    case $arch in arm64|aarch64) arch=arm64 ;; x86_64|amd64) arch=x86_64 ;; *) fail "unsupported macOS architecture: $arch" ;; esac
    platform=macos
    default_url=https://github.com/qu8n/mddog/releases/latest/download
    ;;
  Linux)
    case $arch in arm64|aarch64) arch=aarch64 ;; x86_64|amd64) arch=x86_64 ;; *) fail "unsupported Linux architecture: $arch" ;; esac
    platform=linux
    default_url=https://github.com/liyutongzuishuai/mddogforlinux/releases/latest/download
    ;;
  *) fail "unsupported operating system: $os (use macOS or Linux)" ;;
esac
url=${MDDOG_RELEASE_URL:-$default_url}
url=${url%/}
tarball=mdd-$platform-$arch.tar.gz
for tool in curl tar mktemp sed; do
  command -v "$tool" >/dev/null 2>&1 || fail "$tool is required"
done
if command -v sha256sum >/dev/null 2>&1; then
  checksum=sha256sum
elif command -v shasum >/dev/null 2>&1; then
  checksum=shasum
else
  fail "sha256sum or shasum is required to verify the download"
fi

# Resolve the private folder before the trap can delete it.
tmp=$(mktemp -d) || fail "could not create a temporary folder"
case $tmp in /*) ;; *) tmp="$(pwd -P)/$tmp" ;; esac
[ -d "$tmp" ] && [ ! -L "$tmp" ] || fail "invalid temporary folder"
tmp=$(CDPATH= cd -- "$tmp" && pwd -P) || fail "could not resolve the temporary folder"
[ -n "$tmp" ] && [ "$tmp" != / ] || fail "unsafe temporary folder"
cleanup() { [ -z "$tmp" ] || rm -rf -- "$tmp"; }
trap cleanup 0
trap 'exit 1' HUP INT TERM
cd -- "$tmp"
curl -fsSL -o "$tarball" "$url/$tarball" || fail "could not download $url/$tarball"
curl -fsSL -o "$tarball.sha256" "$url/$tarball.sha256" || fail "could not download the checksum"
case $checksum in
  sha256sum) sha256sum -c "$tarball.sha256" >/dev/null || fail "checksum mismatch" ;;
  shasum) shasum -a 256 -c "$tarball.sha256" >/dev/null || fail "checksum mismatch" ;;
esac
# A release contains one executable, not paths that could escape the temporary folder.
[ "$(tar tzf "$tarball")" = mdd ] || fail "the archive must contain only mdd"
tar xzf "$tarball" || fail "could not extract the archive"
[ -f mdd ] && [ ! -L mdd ] && [ -x mdd ] || fail "the archive has no executable mdd"
version=$(./mdd --version) || fail "the release cannot run on this system (Linux requires glibc 2.39 or newer)"

mkdir -p -- "$dir"
mv -f -- mdd "$dir/mdd"
if [ "$os" = Darwin ]; then xattr -d com.apple.quarantine "$dir/mdd" 2>/dev/null || true; fi
echo "$ok Installed $version to $show/mdd"

# Shell-quote a path instead of putting arbitrary folder names into startup commands.
quote() { printf "'%s'" "$(printf '%s' "$1" | sed "s/'/'\\\\''/g")"; }
quoted_dir=$(quote "$dir")
echo
case ":$PATH:" in
  *":$dir:"*) ;;
  *)
    case ${SHELL:-sh} in
      */fish) add="fish_add_path -- $quoted_dir" ;;
      */bash)
        if [ "$os" = Linux ]; then rc='~/.bashrc'; else rc='~/.bash_profile'; fi
        line="export PATH=$quoted_dir:\"\$PATH\""
        add="printf '%s\\n' $(quote "$line") >> $rc"
        ;;
      */zsh)
        line="export PATH=$quoted_dir:\"\$PATH\""
        add="printf '%s\\n' $(quote "$line") >> ~/.zshrc"
        ;;
      *) add="export PATH=$quoted_dir:\"\$PATH\"" ;;
    esac
    echo "$show is not on your PATH. To add it, run:"
    echo
    echo "    $add"
    echo
    ;;
esac
cat <<'EOF'
Get started:

    mdd             pick a Markdown file to preview
    mdd FILE.md     preview one file
    mdd --help      see all options
EOF
