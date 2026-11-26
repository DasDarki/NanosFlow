FROM rust:1-bookworm AS build
WORKDIR /source
COPY . .
RUN cargo build --release --locked --package nf-relay

FROM debian:bookworm-slim
RUN useradd --system --uid 10001 --home-dir /data relay \
    && mkdir /data \
    && chown relay /data
COPY --from=build /source/target/release/nf-relay /usr/local/bin/nf-relay
USER relay
WORKDIR /data
VOLUME /data
EXPOSE 7900/udp
ENTRYPOINT ["nf-relay", "--identity", "/data/relay-identity"]
