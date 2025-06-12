WEBSITE_ROOT="$PROJECT_ROOT"/website
WEBSITE_BUILD_VARS_DIR="$WEBSITE_ROOT"/build-vars

function _init_website {
	cd "$PROJECT_ROOT/website" || exit
	mkdir -p "$WEBSITE_BUILD_VARS_DIR"
	echo -n "$(get_version)" >"$WEBSITE_BUILD_VARS_DIR"/version
	_copy_default_config
}

function _copy_default_config {
	cp "$PROJECT_ROOT"/crates/tattoy/default_config.toml "$WEBSITE_BUILD_VARS_DIR"/
}

function website-build {
	_init_website
	zola build
}

function website-serve {
	_init_website
	zola serve
}
