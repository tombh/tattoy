#!/bin/bash

BUILD_VARS_DIR=build-vars
WEBSITE_PATH=$(dirname "$(readlink -f "$0")")
cd "$WEBSITE_PATH" || exit

function _init {
	mkdir -p build-vars
	_get_version
	_copy_default_config
}

function _trim {
	sed -e 's/\n//g' | awk '{$1=$1};1'
}

function _get_version {
	version=$(cargo pkgid --manifest-path ../crates/tattoy/Cargo.toml | cut -d "#" -f2 | _trim)
	echo -n "$version" >$BUILD_VARS_DIR/version
}

function _copy_default_config {
	cp ../crates/tattoy/default_config.toml $BUILD_VARS_DIR/
}

function build {
	zola build
}

function serve {
	zola serve
}

_init

subcommand=$1
args=("$@")
"$subcommand" "${args[@]}"
