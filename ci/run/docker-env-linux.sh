#!/bin/bash
set -euo pipefail
IFS=$'\n\t'

PUSH_ORG="rustops"

# Login on Docker Hub when executing on CI
# Redefine long_timeout on CI
if [[ ! -z "${CI+x}" ]]; then
    echo "${DOCKER_PASSWORD}" | base64 --decode | docker login --username "${DOCKER_USERNAME}" --password-stdin

    disable-no-output-timeout() {
        while true; do
            sleep 60
            echo "[disable no-output timeout]"
        done
    }
else
    disable-no-output-timeout() {
        true
    }
fi

build-and-push() {
    docker/env/build.sh "$1"
    docker tag "crater-env-$1" "${PUSH_ORG}/crater-env-$1"
    docker push "${PUSH_ORG}/crater-env-$1"
}

disable-no-output-timeout &
build-and-push mini
build-and-push full
