#!/usr/bin/env bash

set -ex
ROOT_DIR=$(readlink -f $(dirname $0))
cd ${ROOT_DIR}
export PRAVEGA_LTS_PATH=${PRAVEGA_LTS_PATH:-/opt/docker/pravega}
PRUNE_LTS=${PRUNE_LTS:-false}
docker-compose -f docker-compose-pravega.yaml down -v

if [ "$PRUNE_LTS" = true ]; then
   sudo rm -rf ${PRAVEGA_LTS_PATH}
fi

docker-compose -f docker-compose-pravega.yaml up -d
