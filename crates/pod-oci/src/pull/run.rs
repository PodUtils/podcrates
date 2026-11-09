use super::*;

/// Run a pull, writing docker's transcript to `out`.
///
/// ⚠ The transcript is the **terminal state of each layer**, printed once it is
/// known. There is no synthetic progress bar:
/// `docs/conventions/forbidden-patterns.md` forbids a hardcoded or synthetic
/// progress display, and a percentage podvm cannot measure is exactly one.
/// ⭐ `platform` is what the caller asked for, resolved by
/// [`Platform::wanted`] before it gets here so the flag, the environment and
/// the host are settled in one place rather than three.
pub fn pull(
    store: &Store,
    want: &str,
    platform: &Platform,
    policy: &Policy,
    out: &mut dyn Write,
) -> Result<Pulled> {
    let reference = Reference::parse(want)?;

    // ⭐ TODO/image.md T-0213. The refusal is HERE and not in `Reference::parse`,
    // because only here is the transport policy in scope, and a refusal that
    // cannot name the flag that would permit it is a refusal a caller cannot
    // act on.
    if reference.plain_http && !policy.permits_explicit_http(reference.endpoint()) {
        return Err(Error::PlainHttpRefused(format!(
            "{want:?} names http://, and {0} is not configured as an insecure \
             registry. podvm does not downgrade a connection on its own: \
             tcp/80 is black-holed on the runtimes podvm targets, so an \
             automatic fallback hangs rather than failing (T-0201). Permit it \
             deliberately with `--insecure-registry {0}`, with \
             $PODVM_INSECURE_REGISTRIES, or with a line in the registries \
             config file",
            reference.endpoint()
        )));
    }
    // ⛔ Announced once, before anything is fetched. An agent cannot notice a
    // downgraded transport the way a person might.
    if let Some(said) = policy.disclosure(reference.endpoint()) {
        let _ = writeln!(std::io::stderr(), "{said}");
    }
    let probe = crate::probe_cache::resolve(store);

    let mut client = Client::with_policy(policy.clone());
    let endpoint = reference.endpoint().to_string();
    let repository = reference.repository.clone();

    let _ = writeln!(
        out,
        "{}: Pulling from {repository}",
        reference
            .tag
            .clone()
            .unwrap_or_else(|| reference.manifest_selector())
    );

    // ---------------------------------------------------------- the manifest
    let top = client.manifest(&endpoint, &repository, &reference.manifest_selector())?;
    let parsed = Fetched::parse(&top.bytes, Some(&top.media_type))?;

    // ⭐ `resolved` is what the REFERENCE resolved to, and it is the value
    // `docker image inspect` reports in RepoDigests. For a multi-platform tag
    // that is the index digest, not the per-platform manifest digest, and
    // recording the wrong one is exactly the parity M1 is accepted on.
    let resolved = top.digest.clone();
    let (manifest_digest, manifest_bytes, manifest) = match parsed {
        Fetched::Manifest(m) => (resolved.clone(), top.bytes.clone(), m),
        Fetched::Index(index) => {
            let picked = oci::select_platform(&index, platform)?;
            let d = picked.parsed_digest()?;
            let inner = client.manifest(&endpoint, &repository, &d.to_string())?;
            match Fetched::parse(&inner.bytes, Some(&inner.media_type))? {
                Fetched::Manifest(m) => (inner.digest, inner.bytes, m),
                Fetched::Index(_) => {
                    return Err(Error::Oci(format!(
                        "{endpoint}/{repository} served an index where the \
                         {platform} manifest should be. podvm does not follow \
                         an index into another index: an image is one level deep"
                    )))
                }
            }
        }
    };

    // ------------------------------------------------------------- the space
    //
    // ⛔ Before a single layer is fetched. Streaming until ENOSPC leaves a
    // partial store to clean up on a filesystem that is already full, which is
    // the state in which cleanup is least likely to work (T-0203).
    //
    // ⛔ And before the free-space sum, the file-size ceiling, one blob at a
    // time. Free space is shared by the whole pull, but `RLIMIT_FSIZE` binds
    // every file on its own: a blob that declares under the room and over
    // the ceiling would otherwise pull until SIGXFSZ kills the process
    // mid-transfer (T-1342).
    if let Some(ceiling) = space::file_ceiling() {
        let room = space::read(&store.root().to_string_lossy())
            .map(|h| h.free.bytes)
            .unwrap_or(0);
        if let Some(d) = manifest
            .layers
            .iter()
            .chain(std::iter::once(&manifest.config))
            .filter(|d| {
                d.parsed_digest()
                    .map(|x| !store.has_blob(&x))
                    .unwrap_or(true)
            })
            .find(|d| blob_over_ceiling(d.size, Some(ceiling)))
        {
            let digest = d
                .parsed_digest()
                .map(|x| x.to_string())
                .unwrap_or_else(|_| d.digest.clone());
            return Err(Error::NoSpace(format!(
                "blob {digest} declares {} byte(s), over this process's \
                 file-size ceiling of {ceiling} byte(s) ({} free at {}): \
                 nothing was downloaded",
                d.size,
                space::mib(room),
                store.root().display(),
            )));
        }
    }
    let need_bytes: u64 = manifest
        .layers
        .iter()
        .chain(std::iter::once(&manifest.config))
        .filter(|d| {
            d.parsed_digest()
                .map(|x| !store.has_blob(&x))
                .unwrap_or(true)
        })
        .map(|d| d.size)
        .sum::<u64>()
        + top.bytes.len() as u64
        + manifest_bytes.len() as u64;
    // One inode per blob, plus the index, the lock, the cache and the staging
    // file that exists while each blob is in flight.
    let need_inodes = manifest.layers.len() as u64 + 8;
    space::require(
        &store.root().to_string_lossy(),
        &reference.to_string(),
        need_bytes,
        need_inodes,
        STORE_HEADROOM_BYTES,
    )?;

    // ------------------------------------------------------------- the blobs

    for (bytes, d, what) in [
        (&top.bytes, &resolved, "manifest"),
        (&manifest_bytes, &manifest_digest, "manifest"),
    ] {
        if store.has_blob(d) {
            continue;
        }
        // ⛔ The bytes as served. Re-serialising the parsed document produces a
        // different digest and breaks the parity this milestone is accepted on.
        store.put_bytes(bytes, d, what)?;
    }

    // ⚠ The config is fetched with the layers and is NOT announced with them.
    // Driving a real pull on 2026-09-08 showed the config's short digest
    // printed as a third `Pull complete` beside two layers, which reads as an
    // image with three layers. docker's transcript names layers only, and a
    // transcript that names something else is a display that lies.
    let announced = manifest.layers.len();
    let mut present = Vec::new();
    let mut jobs = Vec::new();
    let mut reused = Vec::new();
    for descriptor in manifest
        .layers
        .iter()
        .chain(std::iter::once(&manifest.config))
    {
        let d = descriptor.parsed_digest()?;
        if store.has_blob(&d) {
            present.push(true);
            reused.push(d.to_string());
        } else {
            present.push(false);
            jobs.push(FetchJob {
                digest: d,
                size: descriptor.size,
            });
        }
    }

    // ⭐ TODO/image.md T-0207. One worker per thread, each with its own
    // client: `Client::blob` takes `&mut self`, so a client is not shareable
    // across threads, and each worker builds one from the same policy.
    // ⛔ No `BufWriter` around the sink. T-0214 restarts it on a body that was
    // cut short, and a `BufWriter` has no way to discard what it is holding;
    // the fetch already writes one 128 KiB chunk at a time, so the buffer was
    // adding a copy rather than a saving.
    let report = fetch_parallel(store, &jobs, |job, file, buf, cancel| {
        // ⛔ The flag is read here as well as before staging: another worker
        // may have failed while this one staged, and a blob fetched past the
        // cancellation is a download the pull then throws away.
        if cancel.load(std::sync::atomic::Ordering::Acquire) {
            return Ok(FetchStep::Skipped(file));
        }
        let mut client = Client::with_policy(policy.clone());
        let back = client.blob(
            &endpoint,
            &repository,
            &job.digest,
            Some(job.size),
            file,
            buf,
        )?;
        Ok(FetchStep::Done(back))
    });

    // ⛔ The transcript is assembled HERE, in manifest order, however the
    // fetches interleaved: every worker buffered its lines and this loop
    // prints them by index. A progress display whose order depends on
    // scheduling is a display that reports the machine's mood.
    // ⚠ The completed lines print even where a later blob failed: the
    // sequential loop this replaces printed what it had before returning the
    // error, and the failure-injection leg of
    // `experiments/190-parallel-layers.sh` asserts the
    // order of what is there.
    let mut fetched = Vec::new();
    let mut notes = report.notes.iter();
    for (n, descriptor) in manifest
        .layers
        .iter()
        .chain(std::iter::once(&manifest.config))
        .enumerate()
    {
        let is_layer = n < announced;
        let d = descriptor.parsed_digest()?;
        if present[n] {
            if is_layer {
                let _ = writeln!(out, "{}: Already exists", d.short());
            }
            continue;
        }
        let lines = notes.next().expect("one note per missing blob");
        if let Some(lines) = lines {
            let _ = out.write_all(lines);
        }
        if is_layer {
            let _ = writeln!(out, "{}: Pull complete", d.short());
        }
        fetched.push(d.to_string());
    }
    if let Some(e) = report.error {
        return Err(e);
    }

    // ------------------------------------------------------------ the record
    let config_digest = manifest.config.parsed_digest()?;
    let config: oci::Config = serde_json::from_slice(&store.read_blob(&config_digest)?)
        .map_err(|e| Error::Oci(format!("the image config does not parse: {e}")))?;
    if !config.architecture.is_empty() && !config.os.is_empty() {
        // ⛔ The variant is checked against what was asked for, not assumed
        // from the index entry. `docs/conventions/forbidden-patterns.md`: a
        // cache holding a variant it was not keyed by serves it to the next
        // unqualified fetch, and `Exec format error` is how that surfaces.
        // ⛔ Checked against what was ASKED FOR, not against the host. A
        // deliberate `--platform linux/arm64` on an amd64 machine must not be
        // refused here; a registry serving amd64 bytes under an arm64
        // descriptor must be.
        if !platform.matches(&config.os, &config.architecture, None) {
            return Err(Error::Oci(format!(
                "the config of {reference} declares {}/{}, and podvm asked for \
                 {platform}. The store records the platform of what it holds, \
                 so a mismatch here is refused rather than recorded",
                config.os, config.architecture,
            )));
        }
    }

    let record = store::record_of(
        &reference,
        &resolved,
        &top.media_type,
        &manifest_digest,
        &manifest,
        &config,
        &platform.to_string(),
    )?;
    store.put_record(record.clone())?;
    // ⭐ TODO/image.md T-1321. How the bytes got here, recorded at pull
    // time rather than derived later: the sidecar never lives inside
    // image metadata.
    store.note_pull(&crate::health::Provenance {
        registry: reference.endpoint().to_string(),
        repository: reference.canonical_repository(),
        tag: reference.tag.clone(),
        manifest_digest: manifest_digest.to_string(),
        pulled_at: record.pulled_at.clone(),
        podvm_version: env!("CARGO_PKG_VERSION").to_string(),
    })?;

    let _ = writeln!(out, "Digest: {resolved}");
    let _ = writeln!(
        out,
        "Status: {} for {reference}",
        if fetched.is_empty() {
            "Image is up to date"
        } else {
            "Downloaded newer image"
        }
    );
    Ok(Pulled {
        record,
        reused,
        fetched,
        probe_source: probe.source,
    })
}

/// Pull every offered tag of a repository: `pull -a` (TODO/cli.md T-1331).
///
/// The offer is the registry's tags listing; each tag pulls through
/// `pull`, so per-tag verification, provenance and output are the same
/// as a single pull. A tag with no manifest for this platform is named
/// and skipped: stopping the run on it would make `pull -a` unusable
/// against any repository that also serves another OS. Any other failing
/// tag stops the run naming the tag: a partial set with a green exit
/// would read as everything fetched. An offer with nothing for this
/// platform at all is an error, not an empty success.
pub fn pull_all(
    store: &Store,
    endpoint: &str,
    repository: &str,
    platform: &Platform,
    policy: &Policy,
    out: &mut dyn Write,
) -> Result<Vec<Pulled>> {
    let mut client = Client::with_policy(policy.clone());
    let mut tags = client.tags(endpoint, repository)?;
    tags.sort();
    if tags.is_empty() {
        return Err(Error::Store(format!(
            "{endpoint}/{repository} offers no tags to pull"
        )));
    }
    let _ = writeln!(out, "Pulling {} tag(s) from {repository}", tags.len());
    let mut pulled = Vec::with_capacity(tags.len());
    let mut skipped = 0usize;
    for tag in &tags {
        let want = format!("{endpoint}/{repository}:{tag}");
        match pull(store, &want, platform, policy, out) {
            Ok(p) => pulled.push(p),
            Err(Error::NoPlatform { want, offered }) => {
                let _ = writeln!(
                    out,
                    "tag {tag}: skipped, this index offers no {want} manifest. It offers: {offered}"
                );
                skipped += 1;
            }
            Err(e) => return Err(Error::Store(format!("pull -a stopped at tag {tag}: {e}"))),
        }
    }
    if pulled.is_empty() {
        return Err(Error::Store(format!(
            "{endpoint}/{repository} offers no {platform} manifest under any of its {} tag(s) ({skipped} skipped)",
            tags.len()
        )));
    }
    Ok(pulled)
}
