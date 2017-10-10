#!/bin/bash

usermod -u "$USER_ID" crater
if [ -e /var/run/docker.sock ]; then
    groupmod -g "$DOCKER_GROUP_ID" docker
    usermod -G docker crater
fi
exec su crater -c "/run2.sh $CMD"
