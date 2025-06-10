FROM rust:1-bookworm AS builder

WORKDIR /build

RUN apt-get update && apt-get install -y --no-install-recommends \
    clang && apt-get upgrade -y

COPY ./ ./
ARG BINARY
RUN cargo build --release --bin ${BINARY}


FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    curl \
    && rm -rf /var/lib/apt/lists/* 

ENV TZ=Europe/Oslo
RUN ln -snf /usr/share/zoneinfo/$TZ /etc/localtime && echo $TZ > /etc/timezone

ARG BINARY
COPY --from=builder /build/target/release/${BINARY} /release

CMD ["/release"]
