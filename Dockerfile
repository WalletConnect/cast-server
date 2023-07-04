################################################################################
#
# Build args
#
################################################################################
ARG                 base="rust:buster"
ARG                 runtime="debian:buster-slim"
ARG                 bin="cast-server"
ARG                 version="unknown"
ARG                 sha="unknown"
ARG                 maintainer="WalletConnect"
ARG                 release=""

################################################################################
#
# Install cargo-chef
#
################################################################################
FROM                ${base} AS chef

WORKDIR             /app
RUN                 cargo install cargo-chef

################################################################################
#
# Generate recipe file
#
################################################################################
FROM                chef AS plan

WORKDIR             /app
COPY                Cargo.lock Cargo.toml ./
COPY                src ./src
RUN                 RUSTFLAGS="-C link-arg=-fuse-ld=lld" cargo chef prepare --recipe-path recipe.json

################################################################################
#
# Build the binary
#
################################################################################
FROM                chef AS build

ARG                 release
ENV                 RELEASE=${release:+--release}

# This is a build requirement of `opentelemetry-otlp`. Once the new version
# is rolled out, which no longer requires the `protoc`, we'll be able to
# get rid of this.
RUN                 apt-get update \
    && apt-get install -y --no-install-recommends protobuf-compiler lld llvm

WORKDIR             /app
# Cache dependancies
COPY --from=plan    /app/recipe.json recipe.json
RUN                 RUSTFLAGS="-C link-arg=-fuse-ld=lld" cargo chef cook --recipe-path recipe.json ${RELEASE} --analytics analytics
# Build the local binary
COPY                . .
RUN                 RUSTFLAGS="-C link-arg=-fuse-ld=lld" cargo build --bin cast-server ${RELEASE} --analytics analytics
# Certificate file required to use TLS with AWS DocumentDB.
RUN                 wget https://s3.amazonaws.com/rds-downloads/rds-combined-ca-bundle.pem
################################################################################
#
# Runtime image
#
################################################################################
FROM                ${runtime} AS runtime

ARG                 bin
ARG                 version
ARG                 sha
ARG                 maintainer
ARG                 release
ARG                 binpath=${release:+release}

LABEL               version=${version}
LABEL               sha=${sha}
LABEL               maintainer=${maintainer}

WORKDIR             /app
COPY --from=build   /app/target/${binpath:-debug}/cast-server /usr/local/bin/cast-server
COPY --from=build   /app/rds-combined-ca-bundle.pem /app/rds-combined-ca-bundle.pem
RUN                 apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates libssl-dev \
    && apt-get clean \
    && rm -rf /var/lib/apt/lists/*

USER                1001:1001
ENTRYPOINT          ["/usr/local/bin/cast-server"]
