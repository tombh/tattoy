#!/bin/bash

PROJECT_ROOT=$(dirname "$(readlink -f "$0")")

function _load_includes {
	for file in "$PROJECT_ROOT"/scripts/*.bash; do
		# shellcheck disable=1090
		source "$file"
	done
}

_load_includes

subcommand=$1
args=("$@")
"$subcommand" "${args[@]}"
