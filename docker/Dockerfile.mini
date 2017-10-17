FROM ubuntu:16.04

ENV LAST_UPDATE=2017-05-25
ENV DEBIAN_FRONTEND=noninteractive

RUN apt-get update

# Tools
RUN apt-get install -y gcc pkg-config cmake

# Native deps
RUN apt-get install -y libssl-dev

WORKDIR /source

# Create a crater user that run.sh will map to the value of the host user via
# the USER_ID environment variable, to make the files the container writes not
# be owned by root, but by the running host user
# re https://github.com/docker/docker/issues/7198#issuecomment-158566258
RUN adduser --no-create-home --disabled-login --gecos "" crater --uid 1000

# Configure the user id, controlled by -e USER_ID, and then
# runs some command, controlled by -e CMD
CMD ["bash", "-c", "usermod -u \"$USER_ID\" crater && exec su crater -c \"/run2.sh $CMD\""]

# sets up the environment for $CMD
COPY run2.sh /run2.sh
