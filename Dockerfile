FROM rust:1.45 as planner
WORKDIR /app
RUN cargo install cargo-chef 
COPY . .
RUN cargo chef prepare  --recipe-path recipe.json

FROM rust:1.45 as cacher
WORKDIR /app
RUN cargo install cargo-chef
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json

FROM rust:1.45 as builder
WORKDIR /app
COPY . .
COPY --from=cacher /app/target target
RUN cargo build --release

FROM rust:1.45 as runtime
LABEL authors="Haoyu Lin and Runchao Han"
WORKDIR /app
COPY --from=builder /app/target/release/randchaind /bin/randchaind
COPY --from=builder /app/tools/ /randchain-tools/
# show backtraces
ENV RUST_BACKTRACE=full
ENV RUST_LOG=trace
#      main    test    reg 
# P2P  8333    18333   18444
# RPC  8332    18332   18443
EXPOSE 8333 18333 18444 8332 18332 18443
ENTRYPOINT ["/bin/randchaind"]