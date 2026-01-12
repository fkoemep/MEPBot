FROM rust:1.88-alpine AS builder

WORKDIR /app
COPY . .

RUN apk add --no-cache musl-dev pkgconfig upx ca-certificates

RUN cargo build --release && upx /app/target/release/mep-bot

FROM scratch

WORKDIR /app
ENV PORT=8080

COPY --from=builder /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/
COPY --from=builder /app/target/release/mep-bot ./app
COPY --from=builder /app/*.json ./

EXPOSE 8080

CMD ["./app"]