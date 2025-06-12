function _trim {
	sed -e 's/\n//g' | awk '{$1=$1};1'
}

function get_version {
	cargo pkgid --manifest-path "$PROJECT_ROOT"/crates/tattoy/Cargo.toml | cut -d "#" -f2 | _trim
}
