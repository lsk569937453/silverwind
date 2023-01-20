FROM alpine:latest
COPY ratelimit/target/ratelimit-1.0.jar /etc/axway-ratelimit/ratelimit.jar
RUN chmod go+r /etc/axway-ratelimit/ratelimit.jar
ENTRYPOINT ["java","-Xloggc:/tmp/gc.log","-XX:+UseG1GC","-jar","/etc/axway-ratelimit/ratelimit.jar"]