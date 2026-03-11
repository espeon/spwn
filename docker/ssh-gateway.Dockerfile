# ── builder ───────────────────────────────────────────────────────────────────
FROM golang:1.24-bookworm AS builder

WORKDIR /build

COPY services/go.mod services/go.sum ./
RUN go mod download

COPY services/ ./
RUN CGO_ENABLED=0 GOOS=linux go build -trimpath -ldflags="-s -w" -o spwn-ssh-gateway ./cmd/ssh-gateway

# ── runtime ───────────────────────────────────────────────────────────────────
FROM gcr.io/distroless/static-debian12

COPY --from=builder /build/spwn-ssh-gateway /spwn-ssh-gateway

ENV SSH_GATEWAY_LISTEN_ADDR=0.0.0.0:2222 \
    SSH_GATEWAY_HOST_KEY_PATH=/var/lib/spwn/gateway_host_key \
    CONTROL_PLANE_HTTP_URL=http://control-plane:3019

EXPOSE 2222

ENTRYPOINT ["/spwn-ssh-gateway"]
