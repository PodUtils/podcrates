# pod-actions-store

The actions of a GitHub Actions workflow by tag, for a runner that runs
the workflow on its own host. Each remote `uses: OWNER/REPO[/PATH]@REF` of
the workflow and of each composite action that it names is resolved to a
commit, kept as a tarball in a store, and pinned in a lock file by its
commit and by the SHA-256 of its uncompressed tar. A tag that moved, or a
tarball that differs from the lock, fails the run by name. The host does
each fetch; a unit never fetches an action. The same lock pins each
checkout of another repository (`with: repository`) and each image of the
workflow that a tag names.

podrunner asked for it (PodUtils/podrunner#94) for its local mode
(PodUtils/podrunner@T-118); the reusable workflows of the local mode and
githome need the same resolve and lock. T-564 made it from podrunner's
code; T-587 added podrunner's checkouts and images by tag
(PodUtils/podrunner#99). The crate reads no environment variable and chooses no directory:
the consumer names the lock, the store and the scratch directory of git.

## Use

```rust
use pod_actions_store::{Git, Lock, Mode, Store, add_trees, images, resolve, workflow_trees, workflow_uses};

let workflow = pod_actions_workflow::parse(&text)?.workflow;
let lock = Lock::read(&lock_path)?;                       // no file: an empty lock
let store = Store { dir: store_dir };                     // DIR/OWNER_REPO/SHA.tar.gz
let git = Git::new("https://github.com/", scratch_dir);   // git in PATH
let uses = workflow_uses(&workflow, &["actions/checkout"]);
let r = resolve(&uses, &lock, &store, &git, Mode::Online, "actions.lock")?;
if r.changed {
    r.lock.write(&lock_path)?;                            // sorted, through a temporary file
}
// r.commits: each OWNER/REPO@REF of the run and its commit

// The checkouts of other repositories, after the actions:
let trees = workflow_trees(&workflow, "me/app")?;            // OWNER/REPO@REF, HEAD with no ref
let mut r = add_trees(&trees, r, &store, &git, Mode::Online, "actions.lock")?;

// The images by tag, through the consumer's ImageSource:
let tags = images::workflow_images(&workflow)?;              // NAME:TAG, sorted
let (pins, changed) = images::resolve(&tags, &mut r.lock, &source, Mode::Online, "actions.lock")?;
// pins: ("docker://NAME:TAG", "NAME@sha256:HEX") for each image
```

## API

| Item | What |
| --- | --- |
| `lock::Lock`, `Pin` | `pins`: each `OWNER/REPO@REF` and its commit and `sha256:HEX` digest; `images`: each image `NAME:TAG` and its `sha256:HEX` digest; `parse`, `read` (no file: an empty lock), `render`, `render_with` (the consumer's comment on top), `write`, `write_with` (through a temporary file) |
| `lock::HEADER`, `is_commit` | the comment of `render`; a full commit id (40 hex digits) |
| `store::workflow_uses` | each remote `uses:` of the steps of a `pod_actions_workflow::Workflow`, but the repositories of `except` |
| `store::resolve`, `Resolved` | each reference and each reference of each composite action that it names, against the lock, the store and the forge, until no new reference is left; `commits`, the `lock` after the run, `changed` |
| `store::checkout_target(with, self_repo)` | `OWNER/REPO@REF` of the `with:` of a checkout (`HEAD` when no ref), or `None` for the repository of the run |
| `store::workflow_trees(w, self_repo)` | each checkout of another repository in the steps of the workflow (`actions/checkout@REF` with `with: repository`) |
| `store::add_trees(trees, resolved, store, forge, mode, lock_name)` | each tree resolved, pinned and kept as an action is, with no action file to read |
| `images::ImageSource` | `pull(image)`: the consumer pulls `NAME:TAG` into its store of images and gives the digest `sha256:HEX` |
| `images::by_tag` | an image that a tag names, as `NAME:TAG` (`NAME` alone is `NAME:latest`); `None` for an image by digest |
| `images::workflow_images` | each image of `container:`, `services:` and `docker://` steps that a tag names, sorted, each once |
| `images::resolve(images, lock, source, mode, lock_name)` | each image pinned by the lock and the source: `("docker://NAME:TAG", "NAME@sha256:HEX")`, and whether the lock changed |
| `store::Mode` | `Online` (resolve and fetch what is missing, check each tag of the lock), `Frozen` (refuse a change of the lock, fetch only a tarball that the lock pins), `Offline` (the lock and the store only) |
| `store::Forge` | `resolve(repo, ref)` to a commit (`HEAD`: the default branch), `tar(repo, commit)` (uncompressed, one top directory) |
| `store::Git` (also `Git`) | the forge of git as a program: `base`, `scratch`, `program`; `Git::new(base, scratch)` |
| `store::Store`, `archive_name` | the tarballs `DIR/OWNER_REPO/SHA.tar.gz`, the layout of the archive cache of the runner; `path` |
| `store::digest` | `sha256:HEX` of bytes |
| `Error`, `LineFault`, `CheckoutFault` | each failure with its reference, its commit or its file (below) |

## The rules

* The lock: one line for each reference, `OWNER/REPO@REF COMMIT
  sha256:HEX`, sorted by the reference, so that a change of the lock is a
  small diff. A line that starts with `#` is a comment. A line of another
  form, and a reference that is there twice, fail with the number of the
  line (and the file).
* An image line is `docker://NAME:TAG sha256:HEX`, after the references.
  `NAME:TAG` is the form of `images::by_tag`: a line with no tag or with a
  digest fails, as a lookup never finds it.
* The digest covers the uncompressed tar, so the version of git or of
  gzip does not change it.
* `resolve` reads `action.yml` (or `action.yaml`) at the path of each
  reference, under the top directory of the tar; a composite action adds
  each remote `uses:` of its steps. A local `./` action and a `docker://`
  image are not references.
* A reference by commit takes the commit. A reference by tag or branch in
  the lock: the offline mode takes the commit of the lock; the other modes
  ask the forge and fail when the tag moved (`Error::Moved`).
* A reference that the lock does not have: the online mode adds it; the
  frozen and offline modes refuse it before they ask the forge
  (`Error::NotLocked`).
* The commit of a forge must be a full commit id, as it names a file of
  the store.
* A checkout of another repository (`uses: actions/checkout@REF` with
  `with: repository: OWNER/REPO` and `ref`) is a tree `OWNER/REPO@REF`,
  `HEAD` when no ref. The repository of the run (by any case, or
  `${{ github.repository }}`) is the consumer's own tree. Any other
  expression fails: the host resolves each tree before the first job.
  `add_trees` pins and keeps a tree as an action, with the same modes.
* An image of `container:`, `services:` (a string or `image:`) and a
  `docker://` step that a tag names is pulled and pinned; an image by
  digest is not. An expression fails with its line. A tag in the lock: the
  offline mode takes the digest of the lock; the other modes pull and fail
  when the tag moved (`Error::ImageMoved`). A tag that the lock does not
  have: the online mode adds it; the frozen and offline modes refuse it
  before they pull (`Error::ImageNotLocked`). The digest of a source must
  be `sha256:HEX` of 64 hex digits. The reference keeps the port of a
  registry: `registry.example:5000/tool:1` gives
  `registry.example:5000/tool@sha256:HEX`.
* `Git`: `git ls-remote URL refs/tags/REF refs/tags/REF^{} refs/heads/REF`
  takes the peeled line of an annotated tag first, then the tag, then the
  branch. `HEAD` is `git ls-remote URL HEAD`, the default branch. `tar` makes a directory of its own under `scratch`, does `git
  init`, `git fetch --depth 1 URL COMMIT` and `git archive --format=tar
  --prefix=OWNER-REPO-SHORT/ FETCH_HEAD`, and removes the directory. git
  runs with `GIT_TERMINAL_PROMPT=0` and `GIT_CONFIG_NOSYSTEM=1`, and takes
  the proxy of the host from its environment.

## Errors

| `Error` | When |
| --- | --- |
| `Line` | a line of a lock: the file, the number, `LineFault::Form` or `LineFault::Again` |
| `File` | a file of the lock or of the store: the path and the `io::ErrorKind` |
| `Uses` | a `uses:` that is not `OWNER/REPO[/PATH]@REF` |
| `Forge` | the text of the forge, or a commit that is not a full commit id |
| `Tarball`, `NoAction`, `Action` | a tarball that is not a tar; no `action.yml` at the path; an `action.yml` that does not parse |
| `Moved`, `Pinned` | a tag or a branch that names another commit than the lock; a reference by commit that the lock pins to another commit |
| `NotLocked`, `NotStored` | a reference that the lock does not have, in the frozen or the offline mode; a commit that is not in the store, offline |
| `Digest` | a tarball whose digest is not the digest of the lock |
| `Checkout`, `Tree` | a field of `with:` of a checkout: an expression, not a string, not `OWNER/REPO` (`CheckoutFault`); a tree that is not `OWNER/REPO@REF` |
| `ImageExpression`, `Registry` | an image that is an expression, with its line; the text of the source, a digest that is not `sha256:HEX`, or an image that is not `NAME:TAG` |
| `ImageMoved`, `ImageNotLocked` | an image tag that names another digest than the lock; an image that the lock does not have, in the frozen or the offline mode |

## Parity

Against podrunner `crates/podrunner-local` at `241acb8` (T-564) and
`872b43f` (T-587):

| podrunner | Here |
| --- | --- |
| [x] `src/lock.rs`: `Lock`, `Pin`, `is_commit`, the image lines, and its two tests | `lock.rs`, with the test `a_lock_file_is_written_whole_and_its_errors_name_the_file` (new) |
| [ ] `LOCK_FILE` (`podrunner-actions.lock`) | stays in podrunner: the name of the file is the consumer's |
| [x] `src/store.rs`: `Forge`, `Mode`, `Resolved`, `workflow_uses`, `resolve`, `digest`, `Store` | `store.rs`; `Git` is in `git.rs`, also as `store::Git` |
| [x] `podrunner_broker_fake::actions::archive_name` | `archive_name` |
| [x] `src/store.rs`: `checkout_target`, `workflow_trees`, `add_trees`, and `HEAD` in `Git` | `store.rs`, `git.rs` |
| [x] `src/images.rs`: `ImageSource`, `by_tag`, `workflow_images`, `resolve` | `images.rs` |
| [ ] `images::PodvmPull` (`podvm pull` and `podvm images`) | stays in podrunner: the source of images is the consumer's |
| [x] `src/store/tests.rs`: the eight tests, each with its name | `src/store/tests.rs`, with four tests more |
| [x] `src/images/tests.rs`: the two tests, each with its name | `src/images/tests.rs`, with one test more |
| [x] `tests/store.rs`: the live test | `tests/store.rs`, in the job `live` of `live.yml` |

Differences from the source, each in a call of `docs/decisions.md` (C320
to C324, C376 to C378):

* An error is `Error`, not a `String`; its text names no flag of a command
  (`--frozen`, `--offline`).
* `workflow_uses(w, except)`: the consumer names the repositories that it
  gives in its own way (`&["actions/checkout"]` for podrunner).
* `Git::new(base, scratch)`: the scratch directory is the consumer's, not
  the temporary directory of the system; each fetch makes a directory of
  its own in it.
* SHA-256 of `sha2`, not of `ring`: the same digest.
* The comment on top of a lock is generic; `render_with` and `write_with`
  take the consumer's.
* A commit of a forge that is not a full commit id fails (`Error::Forge`).
* A digest of a source of images that is not `sha256:HEX` fails
  (`Error::Registry`), and so does an image that is not in the form of
  `by_tag`: the lock would not read back. A lock line of an image with no
  tag or with a digest fails.
* `PodvmPull` stays in podrunner; the crate has the `ImageSource` seam.

## Adoption map

| Consumer | Old | New |
| --- | --- | --- |
| podrunner | `podrunner_local::lock::{Lock, Pin, is_commit}` | `pod_actions_store::lock::` with the same names |
| podrunner | `podrunner_local::lock::LOCK_FILE` and the header that names `podrunner run` | podrunner keeps the name; `Lock::write_with(path, HEADER)` with its header |
| podrunner | `podrunner_local::store::{Forge, Mode, Resolved, Store, resolve, digest}` | `pod_actions_store::store::` with the same names |
| podrunner | `podrunner_local::store::Git { base }` | `pod_actions_store::store::Git::new(base, scratch)` |
| podrunner | `workflow_uses(&w)` | `workflow_uses(&w, &["actions/checkout"])` |
| podrunner | an error `String` (`--frozen`, `--offline` in the text) | `pod_actions_store::Error`; podrunner matches `NotLocked { mode, .. }` and `NotStored` for the words of its flags |
| podrunner | `podrunner_local::store::{checkout_target, workflow_trees, add_trees}` | `pod_actions_store::store::` with the same names and parameters; the error is `Error` |
| podrunner | `podrunner_local::images::{ImageSource, by_tag, workflow_images, resolve}` | `pod_actions_store::images::` with the same names and parameters; podrunner matches `ImageNotLocked { mode, .. }` for the words of its flags |
| podrunner | `podrunner_local::images::PodvmPull` | stays in podrunner, as an `ImageSource` |
| podrunner | `podrunner_broker_fake::actions::archive_name` in `Store::path` | `pod_actions_store::archive_name` (the same layout) |
| podrunner | `flate2`, `tar`, `ring` in `crates/podrunner-local/Cargo.toml` for the store | removed when nothing else of the crate needs them |

## Tests

`cargo test -p pod-actions-store`: the lock (sorted, read back, each bad
line by its number, a file written whole); the resolve against a forge in
memory that counts its calls (a composite action by tag, a moved tag, the
offline and frozen modes, a tarball that differs from the lock, the digest
of the uncompressed tar, a forge that gives no commit id); the checkouts of
other repositories (each fault of `with:`, a tree pinned with no action
file, a moved tree); the images by tag against a registry in memory (the
images of a workflow, a moved tag, the modes, a digest that is not
`sha256:HEX`, the lock read back); and the git
forge against a bare repository that each test makes, served by `file://`
(a lightweight tag, an annotated tag, a branch, `HEAD`; the tar; a resolve through
a composite action and back offline). The tests of git skip with the
reason when `git` is not in `PATH`. `tests/store.rs` is a live test
(`#[ignore]`): two actions by tag from github.com.

Plants (`scripts/plants/`): `actions-store-lock-unsorted`,
`actions-store-frozen-reaches-the-forge`,
`actions-store-digest-of-the-gzip`,
`actions-store-forge-commit-unchecked`,
`actions-store-git-tag-not-peeled` (T-564);
`actions-store-lock-images-dropped`,
`actions-store-checkout-expression-taken`,
`actions-store-git-head-unresolved`, `actions-store-image-moved-taken`,
`actions-store-image-digest-unchecked` (T-587).

## Sources

`SOURCES` lists each imported file with its blob: podrunner at `872b43f`.
