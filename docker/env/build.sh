#!/bin/bash

if [[ $# -ne 1 ]]; then
    echo "Usage: $0 <environment>" >&2
    exit 1
fi

docker build -t "crater-env-$1" --target "$1" "$(dirname $(readlink -f "$0"))"
