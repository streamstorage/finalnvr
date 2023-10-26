ARG GSTREAMER_IMAGE=ghcr.io/streamstorage/gstreamer:22.04-1.22.6-0.11.1-dev

FROM ${GSTREAMER_IMAGE}

RUN apt-get update && apt-get install -y libsqlite3-dev wget

RUN cargo install cargo-watch && \
    rustup component add clippy && \
    cargo install diesel_cli --no-default-features --features sqlite

RUN wget -P /tmp https://nodejs.org/download/release/v16.20.2/node-v16.20.2-linux-x64.tar.gz && \
    tar -zxvf /tmp/node-v16.20.2-linux-x64.tar.gz -C /opt/ && \
    rm /tmp/node-v16.20.2-linux-x64.tar.gz

ENV PATH=/opt/node-v16.20.2-linux-x64/bin:$PATH

RUN npm install --global yarn
