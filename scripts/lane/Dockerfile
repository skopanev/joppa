# Lane image for Crew work on Joppa: Rust, the PostgreSQL client, Node and Playwright Chromium.
# The base is Debian 13 with Node 24. The runtime user stays uid 501. Build on the host:
#   docker build -t medulla-crew-joppa:latest -f scripts/lane/Dockerfile scripts/lane
FROM medulla-crew:latest
USER root
ARG RUST_VERSION=1.90.0
ARG PLAYWRIGHT_VERSION=1.64.0
ENV RUSTUP_HOME=/usr/local/rustup \
    CARGO_HOME=/usr/local/cargo \
    PATH=/usr/local/cargo/bin:$PATH \
    RUST_TEST_THREADS=1 \
    PLAYWRIGHT_BROWSERS_PATH=/opt/ms-playwright \
    NODE_PATH=/opt/playwright/node_modules \
    PLAYWRIGHT_MODULE=/opt/playwright/node_modules/playwright
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates curl git build-essential pkg-config postgresql-client \
 && rm -rf /var/lib/apt/lists/*
RUN set -eu; \
    curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal \
      --default-toolchain "$RUST_VERSION" --component clippy --component rustfmt; \
    chown -R 501:501 "$CARGO_HOME" "$RUSTUP_HOME"; \
    cargo --version; rustc --version; cargo clippy --version; cargo fmt --version
RUN set -eu; \
    npm install --prefix /opt/playwright "playwright@${PLAYWRIGHT_VERSION}" --no-audit --no-fund; \
    node /opt/playwright/node_modules/playwright/cli.js install --with-deps chromium; \
    chmod -R a+rX /opt/playwright /opt/ms-playwright; \
    node -e "require(process.env.PLAYWRIGHT_MODULE).chromium.launch().then(b=>b.close())"
USER 501
