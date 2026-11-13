#!/usr/bin/env bash
# Install the v0.2.0-preview.3 GNOUS build into /usr/local.
# Idempotent: backups are refreshed only when the target file actually changed.
set -euo pipefail

src=${1:?用法: install-release.sh <target/release/gnous>}
repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
bin=/usr/local/bin/gnous
desktop=/usr/local/share/applications/gnous.desktop
icon=/usr/local/share/icons/hicolor/scalable/apps/gnous.svg
backup=/usr/local/share/gnous/release-backups
version=$("$src" --version)

[[ "$(id -u)" == 0 ]] || { echo "请以 root 运行"; exit 1; }
[[ "$version" == "gnous 0.2.0-preview.3" ]] || {
    echo "版本不匹配: $version"; exit 1;
}

mkdir -p "$backup"
stamp=$(date +%Y%m%d-%H%M%S)
for target in "$bin" "$desktop" "$icon"; do
    name=$(basename "$target")
    if [[ -f "$target" ]]; then
        cp -a "$target" "$backup/$name.$stamp"
    fi
done

install -m 0755 "$src" "$bin.new" && mv -f "$bin.new" "$bin"
install -m 0644 "$repo/packaging/gnous.desktop" "$desktop.new" && mv -f "$desktop.new" "$desktop"
install -m 0644 "$repo/packaging/gnous.svg" "$icon.new" && mv -f "$icon.new" "$icon"

if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database /usr/local/share/applications || true
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
    gtk-update-icon-cache -f -t /usr/local/share/icons/hicolor || true
fi

echo "installed: $("$bin" --version)"
echo "sha256:    $(sha256sum "$bin" | cut -d' ' -f1)"
echo "desktop:   $desktop"
echo "icon:      $icon"
