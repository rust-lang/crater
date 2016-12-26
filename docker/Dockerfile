FROM ubuntu:15.10

RUN apt-get update

RUN apt-get install gcc -y
RUN apt-get install docker.io -y

# Docker keys and debs
#RUN apt-get install dirmmngr
#RUN apt-get install apt-transport-https ca-certificates -y
#RUN apt-key adv --keyserver hkp://ha.pool.sks-keyservers.net:80 --recv-keys 58118E89F3A912897C070ADBF76221572C52609D
#RUN echo "deb https://apt.dockerproject.org/repo ubuntu-yakkety main" > /etc/apt/sources.list.d/docker.list

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