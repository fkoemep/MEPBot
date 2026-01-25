FROM rust:1.93-alpine AS builder

# Set Cargo/Rust dirs so we can cache them with BuildKit
ENV CARGO_HOME=/usr/local/cargo \
    RUSTUP_HOME=/usr/local/rustup \
    CARGO_TARGET_DIR=/app/target \
    PATH=/usr/local/cargo/bin:$PATH

WORKDIR /app

# Copy only manifests first to leverage layer caching for dependencies

# Install build deps and prepare cache dirs
RUN --mount=type=cache,id=apk,target=/var/cache/apk,sharing=locked,rw \
    apk add musl-dev pkgconfig upx ca-certificates \
    && mkdir -p $CARGO_HOME/registry $CARGO_HOME/git $CARGO_TARGET_DIR

COPY Cargo.toml Cargo.lock ./
# Create a minimal src/main.rs so `cargo fetch` can detect a binary target when running in the builder.
RUN mkdir -p src && printf '%s\n' 'fn main() { }' > src/main.rs
# Now copy the rest of the source and build using the same cache mounts

# Populate the Cargo registry/git caches (requires BuildKit)
RUN --mount=type=cache,id=apptarget,target=/app/target/ \
    --mount=type=cache,id=db,target=/usr/local/cargo/git/db \
    --mount=type=cache,id=registry,target=/usr/local/cargo/registry/ \
    cargo fetch --locked

COPY src/ ./src/


RUN --mount=type=cache,id=apptarget,target=/app/target/ \
    --mount=type=cache,id=db,target=/usr/local/cargo/git/db \
    --mount=type=cache,id=registry,target=/usr/local/cargo/registry/ \
    cargo build --release --locked && upx $CARGO_TARGET_DIR/release/mep-bot && \
    cp $CARGO_TARGET_DIR/release/mep-bot /usr/local/bin/mep-bot

RUN addgroup -S usergroup && adduser -S appuser -G usergroup

FROM scratch

WORKDIR /app
ENV PORT=8080 CARGO_TARGET_DIR=/app/target

COPY --from=builder /etc/passwd /etc/passwd

USER appuser

COPY --from=builder /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/
COPY --from=builder /usr/local/bin/mep-bot ./app
COPY --from=builder /app/*.json ./

EXPOSE 8080

CMD ["./app"]