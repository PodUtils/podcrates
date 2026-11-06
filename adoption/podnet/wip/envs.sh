# The environment of a trial build of podnet. Source it before cargo.
export CARGO_BUILD_JOBS=4
export RUSTUP_TOOLCHAIN=1.99
export CARGO_TARGET_DIR="${XDG_CACHE_HOME:-$HOME/.cache}/podcrates-adoption/target-podnet"
# The incremental cache filled 6.7 GB of disk in one trial.
export CARGO_INCREMENTAL=0
