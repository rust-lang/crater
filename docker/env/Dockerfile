##########################################################
#  Mini image: installs just the basics to do demo runs  #
##########################################################

FROM ubuntu:xenial AS mini

# Install basic dependencies
RUN apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y \
    gcc \
    pkg-config \
    cmake \
    libssl-dev

WORKDIR /source

# Create a crater user that run.sh will map to the value of the host user via
# the USER_ID environment variable, to make the files the container writes not
# be owned by root, but by the running host user
# re https://github.com/docker/docker/issues/7198#issuecomment-158566258
RUN adduser --no-create-home --disabled-login --gecos "" crater --uid 1000

# Configure the user id, controlled by -e USER_ID, and then
# runs some command, controlled by -e CMD
CMD ["bash", "-c", "usermod -u \"$USER_ID\" crater && exec su crater -c \"/run.sh $CMD\""]

# sets up the environment for $CMD
COPY run.sh /run.sh

########################################################
#  Full image: install a bunch of native dependencies  #
########################################################

FROM mini AS full

# Hopefully this pulls in lots of stuff
RUN apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y --install-suggests libgtk-3-dev
