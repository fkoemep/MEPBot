FROM rust:1.93-alpine AS builder

# Set Cargo/Rust dirs so we can cache them with BuildKit
ENV APP_HOME=/app CARGO_HOME=/usr/local/cargo RUSTUP_HOME=/usr/local/rustup CARGO_TARGET_DIR=/app/target PATH=/usr/local/cargo/bin:$PATH

WORKDIR $APP_HOME

RUN addgroup -S usergroup && adduser -S appuser -G usergroup

RUN --mount=type=cache,id=apk,target=/var/cache/apk,sharing=locked,rw apk add musl-dev pkgconfig upx ca-certificates

# Create a minimal src/main.rs so `cargo fetch` can detect a binary target when running in the builder.
RUN mkdir -p src && printf '%s\n' 'fn main() { }' > src/main.rs

COPY Cargo.toml Cargo.lock ./

RUN --mount=type=cache,id=apptarget,target=/app/target/ --mount=type=cache,id=db,target=/usr/local/cargo/git/db --mount=type=cache,id=registry,target=/usr/local/cargo/registry/ cargo build --release --locked

COPY src/ ./src/

RUN --mount=type=cache,id=apptarget,target=/app/target/ --mount=type=cache,id=db,target=/usr/local/cargo/git/db --mount=type=cache,id=registry,target=/usr/local/cargo/registry/ cargo build --release --locked && upx /app/target/release/mep-bot && cp /app/target/release/mep-bot /usr/local/bin/mep-bot

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