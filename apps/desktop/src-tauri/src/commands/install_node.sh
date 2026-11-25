set -eu
ow_node_root="$HOME/.oceanway-config/runtimes/node"
ow_node_bin="$ow_node_root/bin"
ow_node_ok() {
  command -v npm >/dev/null 2>&1 && node -e 'const v=process.versions.node.split(".").map(Number);process.exit(v[0]>24||v[0]===24&&v[1]>=16?0:1)' >/dev/null 2>&1
}
if ! ow_node_ok; then
  export PATH="$ow_node_bin:$PATH"
  if ! ow_node_ok; then
    echo 'Installing the Node.js runtime required by this app...'
    case "$(uname -s)" in Linux) ow_os=linux;; Darwin) ow_os=darwin;; *) echo 'Unsupported Node.js platform' >&2; exit 1;; esac
    case "$(uname -m)" in x86_64|amd64) ow_arch=x64;; arm64|aarch64) ow_arch=arm64;; *) echo 'Unsupported Node.js architecture' >&2; exit 1;; esac
    ow_tmp=$(mktemp -d)
    trap 'rm -rf -- "$ow_tmp"' EXIT
    ow_base=https://nodejs.org/dist/latest-v24.x
    curl --connect-timeout 15 --max-time 60 --retry 2 -fsSL "$ow_base/SHASUMS256.txt" -o "$ow_tmp/SHASUMS256.txt"
    ow_archive=$(awk -v suffix="-$ow_os-$ow_arch.tar.gz" 'index($2,suffix) && substr($2,length($2)-length(suffix)+1)==suffix { print $2; exit }' "$ow_tmp/SHASUMS256.txt")
    case "$ow_archive" in node-v24.*-"$ow_os"-"$ow_arch".tar.gz) ;; *) echo 'No matching official Node.js archive' >&2; exit 1;; esac
    curl --connect-timeout 15 --max-time 300 --retry 2 -fsSL "$ow_base/$ow_archive" -o "$ow_tmp/$ow_archive"
    ow_expected=$(awk -v file="$ow_archive" '$2==file {print $1}' "$ow_tmp/SHASUMS256.txt")
    if command -v sha256sum >/dev/null 2>&1; then ow_actual=$(sha256sum "$ow_tmp/$ow_archive" | awk '{print $1}'); else ow_actual=$(shasum -a 256 "$ow_tmp/$ow_archive" | awk '{print $1}'); fi
    test "$ow_expected" = "$ow_actual" || { echo 'Node.js download checksum mismatch' >&2; exit 1; }
    mkdir -p "$ow_node_root"
    tar -xzf "$ow_tmp/$ow_archive" -C "$ow_node_root" --strip-components=1
    ow_node_ok || { echo 'Installed Node.js runtime cannot start' >&2; exit 1; }
    rm -rf -- "$ow_tmp"
    trap - EXIT
  fi
  export NPM_CONFIG_PREFIX="$ow_node_root"
  # Make the installed apps usable from a newly opened terminal too.
  for ow_profile in "$HOME/.profile" "$HOME/.bashrc" "$HOME/.zshrc"; do
    if ! grep -qF '.oceanway-config/runtimes/node/bin' "$ow_profile" 2>/dev/null; then
      printf '\nexport PATH="$HOME/.oceanway-config/runtimes/node/bin:$PATH"\n' >> "$ow_profile"
    fi
  done
fi
eval "$1"
