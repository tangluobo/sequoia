FROM golang:1.26.9-alpine AS build
WORKDIR /src
COPY go.mod go.sum ./
RUN go mod download
COPY . .
RUN CGO_ENABLED=0 go build -trimpath -ldflags="-s -w" -o /tomato-cloud ./cmd/tomato-cloud \
    && mkdir /data && chown 65532:65532 /data
FROM scratch
COPY --from=build /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/
COPY --from=build /tomato-cloud /tomato-cloud
COPY --from=build --chown=65532:65532 /data /data
USER 65532:65532
WORKDIR /data
EXPOSE 8443
ENTRYPOINT ["/tomato-cloud"]
CMD ["--data", "/data", "--listen", "0.0.0.0:8443", "--behind-proxy"]
