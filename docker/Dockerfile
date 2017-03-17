FROM ubuntu:15.10

RUN apt-get update

# Tools
RUN apt-get install -y gcc
RUN apt-get install -y pkg-config
RUN apt-get install -y cmake

# Native deps
RUN apt-get build-dep -y openssl
RUN apt-get install -y openssl
RUN apt-get build-dep -y libfreetype6
RUN apt-get install -y libfreetype6
RUN apt-get build-dep -y libfontconfig1
RUN apt-get install -y libfontconfig1
RUN apt-get build-dep -y libgtk-3-0
RUN apt-get install -y libgtk-3-0

WORKDIR /source

# Create a cargobomb user that run.sh will map to the value of the host user via
# the USER_ID environment variable, to make the files the container writes not
# be owned by root, but by the running host user
# re https://github.com/docker/docker/issues/7198#issuecomment-158566258
RUN adduser --no-create-home --disabled-login --gecos "" cargobomb --uid 1000

# The run.sh script configures the user id, controlled by -e USER_ID, and then
# runs some command, controlled by -e CMD
COPY run.sh /run.sh
CMD /run.sh

# run.sh script runs run2.sh under su, which sets up the environment and runs
# $CMD
COPY run2.sh /run2.sh