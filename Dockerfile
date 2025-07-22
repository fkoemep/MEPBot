FROM rust:1.88-alpine AS builder

WORKDIR /app
COPY . .

RUN apk add --no-cache musl-dev pkgconfig && cargo build --release

RUN apk add --no-cache upx ca-certificates && upx /app/target/release/mep-bot

FROM scratch

WORKDIR /app
ENV PORT=8080

COPY --from=builder /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/
COPY --from=builder /app/target/release/mep-bot ./app
COPY --from=builder /app/*.json ./

EXPOSE 8080

CMD ["./app"]