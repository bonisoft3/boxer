# syntax = devthefuture/dockerfile-x:v1.3.3@sha256:807e3b9a38aa29681f77e3ab54abaadb60e633dc5a5672940bb957613b4f9c82
FROM ../../cargo#monorepo as src
WORKDIR /monorepo
COPY --from=./../../libraries/xproto/cargo /monorepo /monorepo
COPY services/boxer /monorepo/services/boxer
RUN /root/.cargo/bin/rustup run stable cargo build --target x86_64-unknown-linux-musl --release --bin server
