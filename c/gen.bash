#!/usr/bin/env bash

SCRIPT_DIR=$( cd -- "$( dirname -- "${BASH_SOURCE[0]}" )" &> /dev/null && pwd )

cd $SCRIPT_DIR/../src/proto-public-api

nanopb_generator --output-dir=. public_api_types.proto public_api_up.proto public_api_down.proto

mkdir -p $SCRIPT_DIR/generated/inc
mkdir -p $SCRIPT_DIR/generated/src

mv *.pb.h $SCRIPT_DIR/generated/inc
mv *.pb.c $SCRIPT_DIR/generated/src
