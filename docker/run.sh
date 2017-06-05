#!/bin/bash

usermod -u $USER_ID cargobomb
if [ -e /var/run/docker.sock ]; then
    groupmod -g $DOCKER_GROUP_ID docker
    usermod -G docker cargobomb
fi
exec su cargobomb -c "/run2.sh $CMD"
