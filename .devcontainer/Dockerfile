# See here for image contents: https://github.com/microsoft/vscode-dev-containers/tree/v0.241.1/containers/rust/.devcontainer/base.Dockerfile

# [Choice] Debian OS version (use bullseye on local arm64/Apple Silicon): buster, bullseye
ARG VARIANT="buster"
FROM mcr.microsoft.com/vscode/devcontainers/rust:0-${VARIANT}

# Setup environment variables
ENV DEBIAN_FRONTEND=noninteractive \
    DISPLAY=":1" \
    VNC_PORT="5900" \
    NOVNC_PORT="6080" \
    DISPLAY_WIDTH="1280" \
    DISPLAY_HEIGHT="720"

COPY build.sh /tmp/build.sh
COPY docker-init.sh /usr/local/share/docker-init.sh

RUN bash /tmp/build.sh && \
    rm /tmp/build.sh && \
    chmod +x /usr/local/share/docker-init.sh

ENTRYPOINT [ "/usr/local/share/docker-init.sh" ]