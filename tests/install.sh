#!/bin/sh
# Offline checks of platform assets, checksum tools, and installation failure paths.
set -eu
root=$(mktemp -d)
root=$(CDPATH= cd -- "$root" && pwd -P)
[ -n "$root" ] && [ "$root" != / ] || exit 1
trap 'rm -rf -- "${root:?}"' 0
trap 'exit 1' HUP INT TERM
repo=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd -P)
real_path=$PATH
export MDDOG_TEST_ROOT="$root"
export MDDOG_TEST_ASSETS="$root/assets"
export REAL_TAR="$(command -v tar)"
export REAL_MKTEMP="$(command -v mktemp)"
export REAL_SHA256="$(command -v sha256sum || true)"
export REAL_SHASUM="$(command -v shasum || true)"
[ -n "$REAL_SHA256$REAL_SHASUM" ] || { echo 'a SHA-256 tool is required for the installer tests' >&2; exit 1; }
mkdir -p "$root/assets" "$root/payload" "$root/bin"
cat > "$root/payload/mdd" <<'PAYLOAD'
#!/bin/sh
printf 'mdd 1.3.0\n'
PAYLOAD
chmod +x "$root/payload/mdd"
for asset in mdd-linux-x86_64 mdd-linux-aarch64 mdd-macos-x86_64 mdd-macos-arm64; do
  "$REAL_TAR" czf "$root/assets/$asset.tar.gz" -C "$root/payload" mdd
  if [ -n "$REAL_SHA256" ]; then
    sum=$("$REAL_SHA256" "$root/assets/$asset.tar.gz")
  else
    sum=$("$REAL_SHASUM" -a 256 "$root/assets/$asset.tar.gz")
  fi
  printf '%s  %s.tar.gz\n' "${sum%% *}" "$asset" > "$root/assets/$asset.tar.gz.sha256"
done
for tool in cat cp chmod gzip mkdir mv rm sed; do ln -s "$(command -v "$tool")" "$root/bin/$tool"; done
cat > "$root/bin/uname" <<'MOCK'
#!/bin/sh
case $1 in -s) printf '%s\n' "$MDDOG_TEST_OS" ;; -m) printf '%s\n' "$MDDOG_TEST_ARCH" ;; *) exit 1 ;; esac
MOCK
cat > "$root/bin/sysctl" <<'MOCK'
#!/bin/sh
printf 'sysctl\n' >> "$MDDOG_TEST_LOG"
printf '%s\n' "${MDDOG_TEST_ARM64:-0}"
MOCK
cat > "$root/bin/xattr" <<'MOCK'
#!/bin/sh
printf 'xattr\n' >> "$MDDOG_TEST_LOG"
MOCK
cat > "$root/bin/mktemp" <<'MOCK'
#!/bin/sh
exec "$REAL_MKTEMP" -d "$MDDOG_TEST_ROOT/tmp.XXXXXX"
MOCK
cat > "$root/bin/tar" <<'MOCK'
#!/bin/sh
printf 'tar %s\n' "$*" >> "$MDDOG_TEST_LOG"
exec "$REAL_TAR" "$@"
MOCK
cat > "$root/bin/curl" <<'MOCK'
#!/bin/sh
[ "$1" = -fsSL ] && [ "$2" = -o ] || exit 90
printf 'curl %s\n' "$4" >> "$MDDOG_TEST_LOG"
name=${4##*/}
case ${MDDOG_TEST_FAILURE:-} in
  download) exit 22 ;;
  checksum-download) case $name in *.sha256) exit 22 ;; esac ;;
esac
cp "$MDDOG_TEST_ASSETS/$name" "$3" || exit 22
if [ "${MDDOG_TEST_FAILURE:-}" = checksum ]; then
  case $name in *.sha256) printf '%064d  %s\n' 0 "${name%.sha256}" > "$3" ;; esac
fi
MOCK
cat > "$root/bin/sha256sum" <<'MOCK'
#!/bin/sh
printf 'checksum sha256sum\n' >> "$MDDOG_TEST_LOG"
if [ -n "$REAL_SHA256" ]; then exec "$REAL_SHA256" "$@"; fi
exec "$REAL_SHASUM" -a 256 "$@"
MOCK
cat > "$root/bin/shasum" <<'MOCK'
#!/bin/sh
printf 'checksum shasum\n' >> "$MDDOG_TEST_LOG"
[ "$1" = -a ] && [ "$2" = 256 ] || exit 91
shift 2
if [ -n "$REAL_SHA256" ]; then exec "$REAL_SHA256" "$@"; fi
exec "$REAL_SHASUM" -a 256 "$@"
MOCK
chmod +x "$root/bin/uname" "$root/bin/sysctl" "$root/bin/xattr" "$root/bin/mktemp" "$root/bin/tar" "$root/bin/curl" "$root/bin/sha256sum" "$root/bin/shasum"

fail() { printf 'installer test: %s\n' "$*" >&2; exit 1; }
contains() { case $1 in *"$2"*) ;; *) fail "missing $2" ;; esac; }
absent() { case $1 in *"$2"*) fail "unexpected $2" ;; esac; }
no_temp() {
  for folder in "$root"/tmp.*; do [ ! -e "$folder" ] || fail "temporary folder left behind: $folder"; done
}
run() {
  name=$1
  export MDDOG_TEST_OS=$2 MDDOG_TEST_ARCH=$3 MDDOG_TEST_ARM64=$4 MDDOG_TEST_FAILURE=$5
  export HOME="$root/$name home" SHELL=$6 NO_COLOR=1
  export MDDOG_TEST_LOG="$root/$name.log"
  mkdir -p "$HOME"
  : > "$MDDOG_TEST_LOG"
  unset MDDOG_RELEASE_URL MDDOG_INSTALL_DIR
  if [ -n "$7" ]; then export MDDOG_RELEASE_URL=$7; fi
  if [ -n "$8" ]; then export MDDOG_INSTALL_DIR=$8; fi
  if PATH="$root/bin" /bin/sh "$repo/install.sh" > "$root/$name.out" 2>&1; then status=0; else status=$?; fi
  output=$(PATH="$real_path" sed -n '1,200p' "$root/$name.out")
  log=$(PATH="$real_path" sed -n '1,200p' "$MDDOG_TEST_LOG")
  no_temp
}

run linux-x86 Linux x86_64 0 '' /bin/bash '' ''
[ "$status" = 0 ] || fail "$output"
contains "$log" 'https://github.com/liyutongzuishuai/mddogforlinux/releases/latest/download/mdd-linux-x86_64.tar.gz'
contains "$log" 'checksum sha256sum'
absent "$log" sysctl
absent "$log" xattr
contains "$output" '~/.bashrc'
[ -x "$HOME/.local/bin/mdd" ] || fail 'Linux binary missing'
# The startup hint is executable shell syntax even when the home path contains spaces.
hint=$(PATH="$real_path" sed -n 's/^    \(printf .*\)$/\1/p' "$root/linux-x86.out")
[ -n "$hint" ] || fail 'no bash PATH hint'
/bin/sh -c "$hint"
contains "$(sed -n '1,5p' "$HOME/.bashrc")" "$HOME/.local/bin"

run linux-arm Linux aarch64 0 '' /bin/fish https://offline.invalid/download/ "$root/custom bin"
[ "$status" = 0 ] || fail "$output"
contains "$log" 'https://offline.invalid/download/mdd-linux-aarch64.tar.gz'
contains "$output" fish_add_path
[ -x "$root/custom bin/mdd" ] || fail 'custom installation directory ignored'
run linux-alias Linux arm64 0 '' /bin/zsh '' ''
[ "$status" = 0 ] || fail "$output"
contains "$log" mdd-linux-aarch64.tar.gz
contains "$output" '~/.zshrc'
run mac-intel Darwin x86_64 0 '' /bin/bash '' ''
[ "$status" = 0 ] || fail "$output"
contains "$log" 'https://github.com/qu8n/mddog/releases/latest/download/mdd-macos-x86_64.tar.gz'
contains "$log" xattr
contains "$output" '~/.bash_profile'
run rosetta Darwin x86_64 1 '' /bin/zsh '' ''
[ "$status" = 0 ] || fail "$output"
contains "$log" mdd-macos-arm64.tar.gz

# Hide sha256sum to exercise the shasum fallback without requiring Perl on Linux.
[ "$root/bin/sha256sum" = "$MDDOG_TEST_ROOT/bin/sha256sum" ] || exit 1
mv "$root/bin/sha256sum" "$root/sha256sum.mock"
run fallback Linux amd64 0 '' /bin/bash '' ''
[ "$status" = 0 ] || fail "$output"
contains "$log" 'checksum shasum'
[ "$root/bin/shasum" = "$MDDOG_TEST_ROOT/bin/shasum" ] || exit 1
mv "$root/bin/shasum" "$root/shasum.mock"
run no-checksum Linux x86_64 0 '' /bin/bash '' ''
[ "$status" != 0 ] || fail 'missing checksum tools accepted'
contains "$output" 'sha256sum or shasum is required'
absent "$log" 'curl '
[ "$root/sha256sum.mock" = "$MDDOG_TEST_ROOT/sha256sum.mock" ] || exit 1
mv "$root/sha256sum.mock" "$root/bin/sha256sum"
[ "$root/shasum.mock" = "$MDDOG_TEST_ROOT/shasum.mock" ] || exit 1
mv "$root/shasum.mock" "$root/bin/shasum"

for failure in download checksum-download checksum; do
  run "$failure" Linux x86_64 0 "$failure" /bin/bash '' ''
  [ "$status" != 0 ] || fail "$failure accepted"
  [ ! -e "$HOME/.local/bin/mdd" ] || fail "$failure installed a binary"
  absent "$log" 'tar xzf'
done
contains "$output" 'checksum mismatch'
run unsupported-os FreeBSD x86_64 0 '' /bin/bash '' ''
[ "$status" != 0 ] || fail 'unsupported OS accepted'
contains "$output" 'unsupported operating system'
absent "$log" 'curl '
run unsupported-arch Linux riscv64 0 '' /bin/bash '' ''
[ "$status" != 0 ] || fail 'unsupported architecture accepted'
contains "$output" 'unsupported Linux architecture'
absent "$log" 'curl '
printf 'installer offline tests passed\n'
