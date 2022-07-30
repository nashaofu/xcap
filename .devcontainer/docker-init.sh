#!/usr/bin/env bash

DISPLAY="${DISPLAY:-:1}"
VNC_PORT="${VNC_PORT:-5900}"
NOVNC_PORT="${NOVNC_PORT:-6080}"
DISPLAY_WIDTH="${DISPLAY_WIDTH:-1280}"
DISPLAY_HEIGHT="${DISPLAY_HEIGHT:-720}"

fluxbox &
Xvfb ${DISPLAY} -screen 0 ${DISPLAY_WIDTH}x${DISPLAY_HEIGHT}x24 -dpi 96 -listen tcp -ac &
x11vnc -display ${DISPLAY} -rfbport ${VNC_PORT} -forever &
websockify --web /usr/share/novnc ${NOVNC_PORT} localhost:${VNC_PORT}
