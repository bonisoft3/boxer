# syntax = devthefuture/dockerfile-x:v1.3.3@sha256:807e3b9a38aa29681f77e3ab54abaadb60e633dc5a5672940bb957613b4f9c82
FROM alpine:3.19.0@sha256:51b67269f354137895d43f3b3d810bfacd3945438e94dc5ac55fdac340352f48
# The alpine cross compiled binary trick did not really work, look into it later.
COPY --from=./services/boxer/cargo#src /monorepo/target/x86_64-unknown-linux-musl/release/server /usr/local/bin
ENTRYPOINT ["/usr/local/bin/server"]
