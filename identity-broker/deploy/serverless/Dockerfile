FROM rust:1.92-bookworm AS build
WORKDIR /build
COPY identity-broker/Cargo.toml identity-broker/Cargo.lock identity-broker/
COPY identity-broker/src identity-broker/src
COPY src/identity/crypto.rs src/identity/crypto.rs
RUN cargo build --release --locked --manifest-path identity-broker/Cargo.toml

FROM build AS build-azure
RUN apt-get update && apt-get install -y --no-install-recommends musl-tools && rm -rf /var/lib/apt/lists/*
RUN rustup target add x86_64-unknown-linux-musl
RUN cargo build --release --locked --manifest-path identity-broker/Cargo.toml --target x86_64-unknown-linux-musl

FROM scratch AS azure-package
COPY --from=build-azure /build/identity-broker/target/x86_64-unknown-linux-musl/release/lttl-key-broker /lttl-key-broker
COPY identity-broker/deploy/serverless/azure/ /

FROM debian:bookworm-slim AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=build /build/identity-broker/target/release/lttl-key-broker /app/lttl-key-broker
# Config and optional database CA certificates are mounted at deployment time.
ENV PORT=8080
USER 10001:10001
EXPOSE 8080
ENTRYPOINT ["/app/lttl-key-broker"]
CMD ["/app/config/broker.json"]

FROM runtime AS lambda
COPY --from=public.ecr.aws/awsguru/aws-lambda-adapter:1.1.0 /lambda-adapter /opt/extensions/lambda-adapter
ENV AWS_LWA_PORT=8080 AWS_LWA_READINESS_CHECK_PATH=/health AWS_LWA_ASYNC_INIT=true
# Mounting arbitrary files is not supported by Lambda; package only public config/CA.
COPY identity-broker/deploy/serverless/config/ /app/config/

FROM runtime AS cloud-run
# Override broker.json with a Secret Manager volume at /app/config on Cloud Run.
COPY identity-broker/deploy/serverless/config/ /app/config/
