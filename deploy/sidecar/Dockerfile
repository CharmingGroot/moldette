# Builds the moldette-sidecar image from the repository root:
#   docker build -f deploy/sidecar/Dockerfile .

FROM rust:1.99.0-alpine3.24@sha256:0cce0a5e0e8ba67b455257a3a02a1d99005f382748789d6464460028810f1627 AS build
RUN apk add --no-cache musl-dev
WORKDIR /src
COPY . .
RUN cargo build --release --locked -p moldette-sidecar \
    && cp target/release/moldette-sidecar /moldette-sidecar

FROM gcr.io/distroless/static-debian13:nonroot@sha256:8ce6dd87e9823c5b48dcfb2c8604714ccb351ce59607327bcc4957448da279b5
COPY --from=build /moldette-sidecar /moldette-sidecar
# The nonroot user of the base image, numeric so that Kubernetes can verify
# runAsNonRoot.
USER 65532:65532
ENTRYPOINT ["/moldette-sidecar"]
