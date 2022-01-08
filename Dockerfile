# This Dockerfile is composed of two steps: the first one builds the release
# binary, and then the binary is copied inside another, empty image.

#################
#  Build image  #
#################

FROM ubuntu:focal AS build

RUN apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y \
    ca-certificates \
    curl \
    build-essential \
    git \
    pkg-config \
    libsqlite3-dev \
    libssl-dev

# Install the currently pinned toolchain with rustup
RUN curl https://static.rust-lang.org/rustup/dist/x86_64-unknown-linux-gnu/rustup-init >/tmp/rustup-init && \
    chmod +x /tmp/rustup-init && \
    /tmp/rustup-init -y --no-modify-path --default-toolchain nightly --profile minimal
ENV PATH=/root/.cargo/bin:$PATH

# Build the dependencies in a separate step to avoid rebuilding all of them
# every time the source code changes. This takes advantage of Docker's layer
# caching, and it works by copying the Cargo.{toml,lock} with dummy source code
# and doing a full build with it.
WORKDIR /source
COPY Cargo.lock Cargo.toml /source/
RUN mkdir -p /source/src && \
    echo "fn main() {}" > /source/src/main.rs && \
    echo "fn main() {}" > /source/build.rs

RUN cargo fetch
RUN cargo build --release

# Dependencies are now cached, copy the actual source code and do another full
# build. The touch on all the .rs files is needed, otherwise cargo assumes the
# source code didn't change thanks to mtime weirdness.
RUN rm -rf /source/src /source/build.rs
COPY src /source/src
COPY build.rs /source/build.rs
COPY assets /source/assets
COPY templates /source/templates
COPY .git /source/.git
RUN find /source -name "*.rs" -exec touch {} \; && cargo build --release

##################
#  Output image  #
##################

FROM ubuntu:focal AS binary

RUN apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y \
    docker.io \
    build-essential \
    pkg-config \
    libssl-dev \
    ca-certificates \
    tini

RUN mkdir /workspace
ENV CRATER_WORK_DIR=/workspace
ENV CRATER_INSIDE_DOCKER=1

RUN mkdir /crater
COPY config.toml /crater/config.toml
WORKDIR /crater

COPY --from=build /source/target/release/crater /usr/local/bin/
ENTRYPOINT ["tini", "--", "crater"]
