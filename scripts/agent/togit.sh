#!/bin/sh
# Make each path dependency on a crate of the podcrates tree TREE a git
# dependency at the tag vVERSION, in each manifest of the checkout DIR.
#
#   sh togit.sh DIR VERSION TREE
v=$2
here=$3
cd "$1" || exit 1
for manifest in $(git ls-files '*Cargo.toml'); do
    sed -i -E "s|^(pod-[a-z-]+) = \\{ path = \"$here/crates/pod-[a-z-]+\"|\\1 = { git = \"https://github.com/PodUtils/podcrates\", tag = \"v$v\"|" "$manifest"
    sed -i -E "/^pod-[a-z-]+ = \\{ git = \"https:\\/\\/github.com\\/PodUtils\\/podcrates\"/s|, version = \"[0-9.]+\"|, version = \"$v\"|" "$manifest"
done
