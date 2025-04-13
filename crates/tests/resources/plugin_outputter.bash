#!/bin/bash

SCRIPT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" &>/dev/null && pwd)
TEMP_DIRECTORY="$SCRIPT_DIR/../../../target/tmp"
mkdir -p "$TEMP_DIRECTORY"
MESSAGES_PATH="$TEMP_DIRECTORY/plugin.messages"
touch "$MESSAGES_PATH"

tail -F "$MESSAGES_PATH"
