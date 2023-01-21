FROM ubuntu:latest
COPY rust-proxy/rust-proxy /etc/rust-proxy
RUN chmod go+r /etc/rust-proxy
CMD ["/etc/rust-proxy","--listener","127.0.0.1:9550","--server-addr","localhost:8888"]