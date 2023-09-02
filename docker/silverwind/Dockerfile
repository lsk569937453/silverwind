FROM ubuntu:jammy-20230301
#RUN apt-get update &&\
#    apt-get install -y heaptrack &&\
#    apt-get install -y heaptrack-gui
#RUN apt-get update &&\
#    apt-get install -y valgrind
COPY silverwind /tmp/silverwind
RUN chmod go+r /tmp/silverwind
WORKDIR "/tmp"
ENTRYPOINT ["/tmp/silverwind"]
#ENTRYPOINT ["tail", "-f", "/dev/null"]

