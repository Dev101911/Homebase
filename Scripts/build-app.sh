#!/bin/zsh
# Builds Homebase.app for local testing: the SwiftUI menu bar app with hb and
# hbd inside Contents/MacOS, ad-hoc signed. Release signing, notarization and
# the DMG come with Scripts/release.sh in the 1.0 milestone.
#
# Usage: Scripts/build-app.sh [--debug] [--open]
set -euo pipefail

cd "$(dirname "$0")/.."
config=release
open_app=false
for arg in "$@"; do
  case $arg in
    --debug) config=debug ;;
    --open) open_app=true ;;
    *) echo "usage: Scripts/build-app.sh [--debug] [--open]" >&2; exit 2 ;;
  esac
done

export PATH="$HOME/.cargo/bin:/opt/homebrew/opt/rustup/bin:$PATH"
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)

echo "→ Rust agent and CLI ($config)"
if [[ $config == release ]]; then cargo build --release --quiet; else cargo build --quiet; fi

echo "→ Swift menu bar app ($config)"
swift build --package-path app -c "$config" --quiet
bin=$(swift build --package-path app -c "$config" --show-bin-path)

app=build/Homebase.app
echo "→ Assembling $app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
sed "s/__VERSION__/$version/" app/Resources/Info.plist > "$app/Contents/Info.plist"
cp "$bin/Homebase" "$app/Contents/MacOS/Homebase"
cp "target/$config/hb" "target/$config/hbd" "$app/Contents/MacOS/"

echo "→ Signing (ad hoc)"
codesign --force --sign - --timestamp=none "$app/Contents/MacOS/hb" "$app/Contents/MacOS/hbd"
codesign --force --sign - --timestamp=none "$app"
codesign --verify --strict "$app"

echo "✓ $app ($version)"
if $open_app; then
  open "$app"
fi
