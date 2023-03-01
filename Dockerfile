FROM ubuntu:latest
COPY rust-proxy/rust-proxy /etc/rust-proxy
RUN chmod go+r /etc/rust-proxy
CMD ["/etc/rust-proxy","RUST_LOG=debug","--server-addr","localhost:8888"]