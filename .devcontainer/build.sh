#!/usr/bin/env bash

export DEBIAN_FRONTEND=noninteractive

# Install noVNC X11 packages
apt-get update
apt-get -y install --no-install-recommends \
    fluxbox \
    xvfb \
    x11vnc \
    novnc \
    libxcb1-dev \
    libxrandr-dev \
    libdbus-1-dev

# Configure noVNC
cp /usr/share/novnc/vnc.html /usr/share/novnc/index.html

# Clean
apt-get clean
rm -rf /var/lib/apt/lists/*
