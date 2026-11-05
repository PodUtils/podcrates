# pod-testkit

The test kit of podcrates: a dev-dependency of each crate, and of a
consumer that wants it. It is never a dependency of a release binary.

## API

* `need_loopback!()` and `need_loopback!(value)`: a test that needs TCP or
  UDP on 127.0.0.1 returns at once, with a line `skipping, REASON` on
  stderr, on a host that refuses `bind(AF_INET)` or has no loopback.
* `need_localhost!()`: the same for a host that does not resolve
  `localhost`.
* `host::loopback()`, `host::localhost()`: the probes, each run once for
  each test binary.

A stand-in of this kit speaks bytes and HTTP only, and takes no type of a
pod crate, so that each crate can dev-depend on the kit.

## Adoption map

| Consumer | Old path | New path |
| --- | --- | --- |
| podnet | `test_host::need_loopback!()` (`tests/test_host/mod.rs`, `#[path]`) | `pod_testkit::need_loopback!()` |
| podnet | `test_host::need_localhost!()` | `pod_testkit::need_localhost!()` |
