FROM rust:1.93-alpine AS builder

ENV APP_HOME=/app

WORKDIR $APP_HOME

RUN addgroup -S usergroup && adduser -S appuser -G usergroup

RUN --mount=type=cache,id=apk,target=/var/cache/apk,sharing=locked,rw \
    apk add musl-dev pkgconfig upx ca-certificates

COPY Cargo.toml Cargo.lock ./

# Create a minimal src/main.rs so `cargo fetch` can detect a binary target when running in the builder.
RUN mkdir -p src && printf '%s\n' 'fn main() { }' > dummy.rs

RUN sed -i 's#src/main.rs#dummy.rs#' Cargo.toml

RUN --mount=type=cache,id=db,target=/usr/local/cargo/git/db,rw \
    --mount=type=cache,id=registry,target=/usr/local/cargo/registry/,rw \
    cargo build --release --locked

RUN sed -i 's#dummy.rs#src/main.rs#' Cargo.toml

COPY src/ ./src/

RUN --mount=type=cache,id=db,target=/usr/local/cargo/git/db,rw \
    --mount=type=cache,id=registry,target=/usr/local/cargo/registry/,rw \
    cargo build --release --locked

RUN upx $APP_HOME/target/release/app

FROM scratch AS runner

ENV APP_HOME=/app PORT=8080

WORKDIR $APP_HOME

COPY --from=builder /etc/passwd /etc/passwd

USER appuser

COPY --from=builder /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/
COPY --from=builder $APP_HOME/target/release/app ./app
COPY --from=builder $APP_HOME/*.json ./

EXPOSE 8080

ENTRYPOINT ["./app"]