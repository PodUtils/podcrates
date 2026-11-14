//! The images of a run by tag. Before the first job, the consumer collects
//! each image of `container:`, `services:` and `docker://` steps that a tag
//! names ([`workflow_images`]); [`resolve`] pulls each one through an
//! [`ImageSource`] into the consumer's store of images and pins it by the
//! lock: the consumer gives the runner `NAME@sha256:HEX`, never the tag, and
//! a unit never pulls. A tag that moved since the lock fails by name.

use std::collections::BTreeSet;

use pod_actions_workflow::{Kind, Node, Workflow};

use crate::error::Error;
use crate::lock::{Lock, is_digest};
use crate::store::Mode;

/// A source of images: it pulls `NAME:TAG` into the consumer's store and
/// gives its digest `sha256:HEX`. An error is text for the user;
/// [`resolve`] gives it as [`Error::Registry`].
pub trait ImageSource {
    fn pull(&self, image: &str) -> Result<String, String>;
}

/// The image of a `container:` or of a service: the string, or `image:` of
/// the mapping.
fn image_of(node: &Node) -> Option<&str> {
    match &node.kind {
        Kind::String(s) => Some(s),
        Kind::Map(_) => match &node.get("image")?.kind {
            Kind::String(s) => Some(s.as_str()),
            _ => None,
        },
        _ => None,
    }
}

/// An image that a tag names: no digest. `NAME` alone is `NAME:latest`, as
/// docker reads it; the port of a registry is not a tag.
pub fn by_tag(image: &str) -> Option<String> {
    if image.contains('@') || image.trim().is_empty() {
        return None;
    }
    let last = image.rsplit('/').next().unwrap_or(image);
    Some(if last.contains(':') { image.to_string() } else { format!("{image}:latest") })
}

/// Each image of the workflow that a tag names, sorted, each once. An
/// expression is refused: the host pulls each image before the first job.
pub fn workflow_images(w: &Workflow) -> Result<Vec<String>, Error> {
    let mut out = BTreeSet::new();
    let mut add = |image: &str, line: u32| -> Result<(), Error> {
        if image.contains("${{") {
            return Err(Error::ImageExpression { image: image.to_string(), line });
        }
        if let Some(t) = by_tag(image) {
            out.insert(t);
        }
        Ok(())
    };
    for job in &w.jobs {
        if let Some(c) = &job.container
            && let Some(i) = image_of(c)
        {
            add(i, c.mark.line)?;
        }
        if let Some(Node { kind: Kind::Map(services), .. }) = &job.services {
            for (_, s) in services {
                if let Some(i) = image_of(s) {
                    add(i, s.mark.line)?;
                }
            }
        }
        for step in &job.steps {
            if let Some(i) = step.uses.as_deref().and_then(|u| u.strip_prefix("docker://")) {
                add(i, step.mark.line)?;
            }
        }
    }
    Ok(out.into_iter().collect())
}

/// `NAME` of `NAME:TAG`: the tag is after the last `:` of the last part of
/// the path, so the port of a registry stays.
fn name_of(image: &str) -> &str {
    match image.rsplit_once(':') {
        Some((n, tag)) if !n.is_empty() && !tag.contains('/') => n,
        _ => image,
    }
}

/// Pin each image of `images` (`NAME:TAG`, as [`by_tag`] gives it) by the
/// lock and the source; the lock changes for a new image. The result maps
/// `docker://NAME:TAG` to `NAME@sha256:HEX`, and tells whether the lock
/// changed. `lock_name` names the lock file in each error.
pub fn resolve(
    images: &[String],
    lock: &mut Lock,
    source: &dyn ImageSource,
    mode: Mode,
    lock_name: &str,
) -> Result<(Vec<(String, String)>, bool), Error> {
    let pull = |image: &String| -> Result<String, Error> {
        let d = source.pull(image).map_err(|reason| Error::Registry { image: image.clone(), reason })?;
        // The digest goes into the lock and into a reference: take only the
        // form that the lock reads back.
        if !is_digest(&d) {
            return Err(Error::Registry { image: image.clone(), reason: format!("`{d}` is not sha256:HEX") });
        }
        Ok(d)
    };
    let mut out = Vec::new();
    let mut changed = false;
    for image in images {
        // The lock reads back only the form of `by_tag`.
        if by_tag(image).as_ref() != Some(image) {
            let reason = "not NAME:TAG; `by_tag` gives the form of the lock".to_string();
            return Err(Error::Registry { image: image.clone(), reason });
        }
        let not_locked = || Error::ImageNotLocked { image: image.clone(), lock: lock_name.to_string(), mode };
        let digest = match (lock.images.get(image).cloned(), mode) {
            (Some(d), Mode::Offline) => d,
            (Some(d), _) => {
                let now = pull(image)?;
                if now != d {
                    return Err(Error::ImageMoved {
                        image: image.clone(),
                        lock: lock_name.to_string(),
                        pinned: d,
                        now,
                    });
                }
                d
            }
            (None, Mode::Offline | Mode::Frozen) => return Err(not_locked()),
            (None, Mode::Online) => {
                let d = pull(image)?;
                lock.images.insert(image.clone(), d.clone());
                changed = true;
                d
            }
        };
        out.push((format!("docker://{image}"), format!("{}@{digest}", name_of(image))));
    }
    Ok((out, changed))
}

#[cfg(test)]
mod tests;
