#!/usr/bin/env bash
#
# install-linux-deps.sh - the Debian / Ubuntu packages CI builds need.
#
# Usage:
#   scripts/ci/install-linux-deps.sh [--lv2]
#
# Always: the X11 / Wayland / GL / Vulkan / audio / font headers truce's
# GUI and standalone crates link against. --lv2 adds lilv, its tools and
# the LV2 spec, which scripts/ci/lv2-check.sh validates and loads bundles
# with.

set -euo pipefail

packages=(
    build-essential pkg-config
    libx11-dev libx11-xcb-dev libxcb1-dev libxcb-dri2-0-dev libxcb-icccm4-dev libxcursor-dev
    libxkbcommon-dev libxkbcommon-x11-dev libxrandr-dev
    libgl1-mesa-dev libegl1-mesa-dev libvulkan-dev mesa-vulkan-drivers
    libwayland-dev
    libasound2-dev libjack-jackd2-dev
    libfontconfig1-dev libfreetype-dev
)

case "${1:-}" in
    --lv2) packages+=(liblilv-dev lilv-utils lv2-dev sordi) ;;
    "") ;;
    *) echo "usage: scripts/ci/install-linux-deps.sh [--lv2]" >&2; exit 2 ;;
esac

sudo apt-get update
sudo apt-get install -y --no-install-recommends "${packages[@]}"
