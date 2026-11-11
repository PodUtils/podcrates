//! The module set of a guest kernel from its modloop: the squashfs image of
//! `/lib/modules` that a distribution ships beside its kernel (Alpine's
//! `modloop-virt`). The guest cannot mount it when `squashfs.ko` is in the
//! image only, so the host reads it ([`crate::squashfs`]) and gives the guest
//! the modules that it needs, with a `modules.dep` of the set.
//!
//! [`modules`] takes the names of the modules, finds each one in the
//! `modules.dep` of the release, and adds each dependency, as `modprobe`
//! does: each module once, in load order (dependencies before dependents).
//! A name that the kernel has built in (`modules.builtin`) needs no file.

#[cfg(test)]
mod tests;

use std::collections::{HashMap, HashSet};
use std::io::{Read, Seek};

use pod_core::{Class, Failure, Stage};

use crate::Result;
use crate::squashfs::{self, Image, Kind};

/// The ceiling of the bytes of a module set, checked before a module is
/// read.
pub const MAX_SET: u64 = 256 << 20;

/// The places of the directory of a release, from the root of the image: a
/// modloop holds `modules/RELEASE`, a root file system `lib/modules/RELEASE`.
pub const RELEASE_DIRS: [&str; 2] = ["modules", "lib/modules"];

/// One module of a set: its path in `modules.dep`, its name, its bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Module {
    /// The path from the directory of the release (`kernel/net/tun.ko`).
    pub path: String,
    /// The name of the module, with `_` for `-` (`tun`).
    pub name: String,
    pub bytes: Vec<u8>,
}

/// The modules that a guest needs, with their dependencies.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModuleSet {
    /// The kernel release, the name of the directory of the set.
    pub release: String,
    /// Each module once, in load order: each dependency before its
    /// dependents.
    pub modules: Vec<Module>,
    /// The names that the kernel has built in: no file and no load.
    pub builtin: Vec<String>,
    /// The `modules.dep` of the set, in the form of depmod: one line for
    /// each module, in load order.
    pub dep: String,
}

impl ModuleSet {
    /// The sum of the bytes of the modules.
    pub fn size(&self) -> u64 {
        self.modules.iter().map(|m| m.bytes.len() as u64).sum()
    }
}

/// Why a module set does not come from an image: the cause of the
/// [`Failure`] of [`modules`]. A fault of the image is a
/// [`squashfs::Fault`]. [`fault`] finds it in a failure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fault {
    /// A release that is not one part of a path, or whose directory or
    /// `modules.dep` the image does not hold.
    Release(String),
    /// A name that `modules.dep` does not list and that is not built in.
    Unknown(String),
    /// A line of `modules.dep` (from 1) that does not parse.
    Dep { line: usize, why: &'static str },
    /// A file of the release that is not UTF-8 text, by its path.
    NotText(String),
    /// A path of `modules.dep` that is not relative, or that has an empty
    /// part, `.` or `..`.
    Path(String),
    /// A dependency loop through this module.
    Loop(String),
    /// A set larger than its ceiling.
    TooLarge { size: u64, limit: u64 },
}

impl std::fmt::Display for Fault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Fault::Release(r) => write!(f, "no modules.dep of the release {r:?} in the image"),
            Fault::Unknown(name) => write!(f, "{name}: not a module of modules.dep, and not built in"),
            Fault::Dep { line, why } => write!(f, "modules.dep, line {line}: {why}"),
            Fault::NotText(path) => write!(f, "{path}: not UTF-8 text"),
            Fault::Path(path) => write!(f, "modules.dep names {path:?}, which is not a plain relative path"),
            Fault::Loop(path) => write!(f, "a dependency loop through {path}"),
            Fault::TooLarge { size, limit } => write!(f, "a set of {size} bytes, over the {limit}-byte ceiling"),
        }
    }
}

impl std::error::Error for Fault {}

impl Fault {
    /// The failure of this fault: [`Class::Usage`] for a release name that
    /// is not one part, [`Class::NotFound`] for a release or a name that is
    /// not there, [`Class::Policy`] for the ceiling, [`Class::Protocol`] for
    /// a `modules.dep` that does not parse.
    pub fn failure(self) -> Failure {
        let class = match &self {
            Fault::Release(r) if !is_part(r) => Class::Usage,
            Fault::Release(_) | Fault::Unknown(_) => Class::NotFound,
            Fault::TooLarge { .. } => Class::Policy,
            Fault::Dep { .. } | Fault::NotText(_) | Fault::Path(_) | Fault::Loop(_) => Class::Protocol,
        };
        Failure::caused(Stage::Host, class, format!("modloop: {self}"), self)
    }
}

impl From<Fault> for Failure {
    fn from(f: Fault) -> Failure {
        f.failure()
    }
}

/// The [`Fault`] in the chain of `f`, when the set did not come.
pub fn fault(f: &Failure) -> Option<&Fault> {
    crate::cause_of(f)
}

/// The module set of `names` and of their dependencies, from the modloop
/// `modloop` of the kernel `kernel_release`. The image is read in place:
/// the superblock, the tables that the walk needs, `modules.dep`,
/// `modules.builtin` and the blocks of each module of the set.
pub fn modules<R, I>(modloop: R, kernel_release: &str, names: I) -> Result<ModuleSet>
where
    R: Read + Seek,
    I: IntoIterator,
    I::Item: AsRef<str>,
{
    modules_within(modloop, kernel_release, names, MAX_SET)
}

/// [`modules`] with the ceiling `limit` of the set.
fn modules_within<R, I>(modloop: R, kernel_release: &str, names: I, limit: u64) -> Result<ModuleSet>
where
    R: Read + Seek,
    I: IntoIterator,
    I::Item: AsRef<str>,
{
    if !is_part(kernel_release) {
        return Err(Fault::Release(kernel_release.to_string()).into());
    }
    let mut image = Image::open(modloop)?;
    let base = release_dir(&mut image, kernel_release)?;
    let dep =
        text(&mut image, &format!("{base}/modules.dep"))?.ok_or_else(|| Fault::Release(kernel_release.to_string()))?;
    let table = Table::parse(&dep)?;
    let builtin: HashSet<String> = match text(&mut image, &format!("{base}/modules.builtin"))? {
        Some(t) => t.lines().map(str::trim).filter(|l| !l.is_empty()).map(name_of).collect(),
        None => HashSet::new(),
    };
    let (mut roots, mut built) = (Vec::new(), Vec::new());
    for given in names {
        let name = given.as_ref().replace('-', "_");
        match table.by_name.get(&name) {
            Some(&i) => roots.push(i),
            None if builtin.contains(&name) => {
                if !built.contains(&name) {
                    built.push(name);
                }
            }
            None => return Err(Fault::Unknown(given.as_ref().to_string()).into()),
        }
    }
    let order = table.order(&roots)?;
    let mut files = Vec::with_capacity(order.len());
    let mut size = 0u64;
    for &i in &order {
        let file = image.file_at(&format!("{base}/{}", table.paths[i]))?;
        size = size.saturating_add(file.size());
        if size > limit {
            return Err(Fault::TooLarge { size, limit }.into());
        }
        files.push(file);
    }
    let mut modules = Vec::with_capacity(order.len());
    let mut dep = String::new();
    for (&i, file) in order.iter().zip(&files) {
        let path = &table.paths[i];
        dep.push_str(path);
        dep.push(':');
        for &d in &table.deps[i] {
            dep.push(' ');
            dep.push_str(&table.paths[d]);
        }
        dep.push('\n');
        modules.push(Module { path: path.clone(), name: name_of(path), bytes: image.read(file)? });
    }
    Ok(ModuleSet { release: kernel_release.to_string(), modules, builtin: built, dep })
}

/// Whether `s` is one part of a path: not empty, `.` or `..`, and with no
/// `/` and no NUL.
fn is_part(s: &str) -> bool {
    !s.is_empty() && s != "." && s != ".." && !s.contains(['/', '\0'])
}

/// The directory of the release: the first of [`RELEASE_DIRS`] that holds
/// it. A part of a place that is not there, or not a directory, sends the
/// search to the next place.
fn release_dir<R: Read + Seek>(image: &mut Image<R>, release: &str) -> Result<String> {
    for dir in RELEASE_DIRS {
        let path = format!("{dir}/{release}");
        match image.lookup(&path) {
            Ok(inode) if matches!(inode.kind, Kind::Directory(_)) => return Ok(path),
            Ok(_) => {}
            Err(e)
                if matches!(
                    squashfs::fault(&e),
                    Some(squashfs::Fault::Missing(_) | squashfs::Fault::NotADirectory(_))
                ) => {}
            Err(e) => return Err(e),
        }
    }
    Err(Fault::Release(release.to_string()).into())
}

/// The text of the file at `path`, or `None` when the image does not hold
/// it.
fn text<R: Read + Seek>(image: &mut Image<R>, path: &str) -> Result<Option<String>> {
    let file = match image.file_at(path) {
        Ok(file) => file,
        Err(e) if matches!(squashfs::fault(&e), Some(squashfs::Fault::Missing(_))) => return Ok(None),
        Err(e) => return Err(e),
    };
    let bytes = image.read(&file)?;
    String::from_utf8(bytes).map(Some).map_err(|_| Fault::NotText(path.to_string()).into())
}

/// The suffixes of a module file: plain, or compressed for kmod.
const SUFFIXES: [&str; 4] = [".ko", ".ko.gz", ".ko.xz", ".ko.zst"];

/// The name of a module from its path: the file name with no suffix of
/// [`SUFFIXES`], with `_` for `-`, as kmod compares names.
fn name_of(path: &str) -> String {
    let file = path.rsplit('/').next().unwrap_or(path);
    let stem = SUFFIXES.iter().find_map(|s| file.strip_suffix(s)).unwrap_or(file);
    stem.replace('-', "_")
}

/// The table of `modules.dep`: each path, its dependencies by index, and
/// the index of each name. A dependency with no line of its own is a module
/// with no dependency, as modprobe reads it.
#[derive(Debug, Default)]
struct Table {
    paths: Vec<String>,
    deps: Vec<Vec<usize>>,
    /// Whether the path has a line of its own.
    lined: Vec<bool>,
    by_path: HashMap<String, usize>,
    by_name: HashMap<String, usize>,
}

impl Table {
    fn parse(text: &str) -> std::result::Result<Table, Fault> {
        let mut t = Table::default();
        for (n, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let Some((path, rest)) = line.split_once(':') else {
                return Err(Fault::Dep { line: n + 1, why: "no colon" });
            };
            let i = t.index(path.trim())?;
            if std::mem::replace(&mut t.lined[i], true) {
                return Err(Fault::Dep { line: n + 1, why: "a second line of one module" });
            }
            let deps = rest.split_ascii_whitespace().map(|d| t.index(d)).collect::<std::result::Result<Vec<_>, _>>()?;
            t.deps[i] = deps;
        }
        Ok(t)
    }

    /// The index of `path`, new or known. The first path of a name keeps
    /// the name.
    fn index(&mut self, path: &str) -> std::result::Result<usize, Fault> {
        if let Some(&i) = self.by_path.get(path) {
            return Ok(i);
        }
        if path.starts_with('/') || !path.split('/').all(is_part) {
            return Err(Fault::Path(path.to_string()));
        }
        let i = self.paths.len();
        self.paths.push(path.to_string());
        self.deps.push(Vec::new());
        self.lined.push(false);
        self.by_path.insert(path.to_string(), i);
        self.by_name.entry(name_of(path)).or_insert(i);
        Ok(i)
    }

    /// The load order of `roots` and of their dependencies: a walk in depth
    /// with a stack of its own, so that a long chain does not exhaust the
    /// stack of the thread. The dependencies of a line are taken from its
    /// end, as modprobe loads them.
    fn order(&self, roots: &[usize]) -> std::result::Result<Vec<usize>, Fault> {
        const NEW: u8 = 0;
        const OPEN: u8 = 1;
        const DONE: u8 = 2;
        let mut state = vec![NEW; self.paths.len()];
        let mut order = Vec::new();
        for &root in roots {
            if state[root] == DONE {
                continue;
            }
            state[root] = OPEN;
            let mut stack = vec![(root, 0usize)];
            while let Some((node, k)) = stack.last_mut() {
                let deps = &self.deps[*node];
                if *k < deps.len() {
                    let d = deps[deps.len() - 1 - *k];
                    *k += 1;
                    match state[d] {
                        NEW => {
                            state[d] = OPEN;
                            stack.push((d, 0));
                        }
                        OPEN => return Err(Fault::Loop(self.paths[d].clone())),
                        _ => {}
                    }
                } else {
                    state[*node] = DONE;
                    order.push(*node);
                    stack.pop();
                }
            }
        }
        Ok(order)
    }
}
