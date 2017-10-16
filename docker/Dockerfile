FROM ubuntu:16.04

ENV LAST_UPDATE=2017-05-25
ENV DEBIAN_FRONTEND=noninteractive

RUN apt-get update

# Tools
RUN apt-get install -y gcc pkg-config cmake

# Native deps
RUN apt-get install -y libssl-dev
# Hopefully this pulls in lots of stuff
RUN apt-get install -y --install-suggests libgtk-3-dev

WORKDIR /source

# Create a crater user that run.sh will map to the value of the host user via
# the USER_ID environment variable, to make the files the container writes not
# be owned by root, but by the running host user
# re https://github.com/docker/docker/issues/7198#issuecomment-158566258
RUN adduser --no-create-home --disabled-login --gecos "" crater --uid 1000

# The run.sh script configures the user id, controlled by -e USER_ID, and then
# runs some command, controlled by -e CMD
COPY run.sh /run.sh
CMD ["/run.sh"]

# run.sh script runs run2.sh under su, which sets up the environment and runs
# $CMD
COPY run2.sh /run2.sh
