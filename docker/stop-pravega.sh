#!/usr/bin/env bash

set -ex
ROOT_DIR=$(readlink -f $(dirname $0))
cd ${ROOT_DIR}
export PRAVEGA_LTS_PATH=${PRAVEGA_LTS_PATH:-/opt/docker/pravega}
docker-compose -f docker-compose-pravega.yaml down -v
