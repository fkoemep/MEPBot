FROM rust:1.93-alpine AS builder

# Set Cargo/Rust dirs so we can cache them with BuildKit
ENV CARGO_HOME=/usr/local/cargo \
    RUSTUP_HOME=/usr/local/rustup \
    CARGO_TARGET_DIR=/app/target \
    PATH=/usr/local/cargo/bin:$PATH

WORKDIR /app

# Copy only manifests first to leverage layer caching for dependencies

# Install build deps and prepare cache dirs
RUN --mount=type=cache,id=apk,target=/var/cache/apk,rw \
    apk add --no-cache musl-dev pkgconfig upx ca-certificates \
    && mkdir -p $CARGO_HOME/registry $CARGO_HOME/git $CARGO_TARGET_DIR

COPY Cargo.toml Cargo.lock ./
# Create a minimal src/main.rs so `cargo fetch` can detect a binary target when running in the builder.
RUN mkdir -p src && printf '%s\n' 'fn main() { }' > src/main.rs
# Now copy the rest of the source and build using the same cache mounts

# Populate the Cargo registry/git caches (requires BuildKit)
RUN --mount=type=cache,target=/usr/local/cargo/registry/index,rw \
    --mount=type=cache,target=/usr/local/cargo/registry/cache,rw \
    --mount=type=cache,target=/usr/local/cargo/git/db,rw \
    cargo fetch --locked

COPY src/ ./src/


RUN --mount=type=cache,target=/usr/local/cargo/registry/index,rw \
    --mount=type=cache,target=/usr/local/cargo/registry/cache,rw \
    --mount=type=cache,target=/usr/local/cargo/git/db,rw \
    cargo build --release --locked && upx $CARGO_TARGET_DIR/release/mep-bot

FROM scratch

WORKDIR /app
ENV PORT=8080 CARGO_TARGET_DIR=/app/target

COPY --from=builder /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/
COPY --from=builder $CARGO_TARGET_DIR/release/mep-bot ./app
COPY --from=builder /app/*.json ./

EXPOSE 8080

CMD ["./app"]