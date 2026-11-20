//! The entries of `.dynamic` that name a string, and the search of the
//! loader for the needs of one object: `DT_NEEDED`, `DT_RUNPATH` and
//! `DT_RPATH`.

use super::*;

pub(super) const DT_NEEDED: i64 = 1;
pub(super) const DT_SONAME: i64 = 14;
const DT_RPATH: i64 = 15;
const DT_RUNPATH: i64 = 29;

/// Each entry of `.dynamic` whose value is an offset into its string table
/// (`DT_NEEDED`, `DT_SONAME`, `DT_RPATH`, `DT_RUNPATH`), as the tag and the
/// string, in the order of the file, up to `DT_NULL`. An offset outside the
/// table gives no entry.
pub(super) fn dynamic_strings(r: &Image<'_>, sections: &Sections) -> Vec<(i64, String)> {
    let mut out = Vec::new();
    let Some(dynamic) = sections.get(".dynamic") else { return out };
    let strs = sections.by_index(dynamic.link).map(|s| r.bytes(s)).unwrap_or_default();
    let step = if r.class64 { 16 } else { 8 };
    let body = r.bytes(dynamic);
    let mut at = 0usize;
    while at + step <= body.len() {
        let d = r.view(&body[at..]);
        let (tag, val) = if r.class64 {
            (d.u64(0).unwrap_or(0) as i64, d.u64(8).unwrap_or(0))
        } else {
            (d.u32(0).unwrap_or(0) as i32 as i64, u64::from(d.u32(4).unwrap_or(0)))
        };
        match tag {
            0 => break, // DT_NULL
            DT_NEEDED | DT_SONAME | DT_RPATH | DT_RUNPATH => {
                if let Some(s) = cstr_at(&strs, val) {
                    out.push((tag, s));
                }
            }
            _ => {}
        }
        at += step;
    }
    out
}

/// What the loader reads of one object to load its needs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Search {
    /// Where it was read from.
    pub path: String,
    /// `e_machine`: the loader passes over a library of another machine.
    pub machine: u16,
    /// `DT_NEEDED`, in the order of the file.
    pub needed: Vec<String>,
    /// `DT_RUNPATH`, the list as the file holds it (`:` between entries).
    pub runpath: Option<String>,
    /// `DT_RPATH`, the list as the file holds it.
    pub rpath: Option<String>,
}

impl Search {
    /// Read `path` with a bounded read: the header, the section table,
    /// `.dynamic` and its strings. The symbols are not read.
    pub fn read(path: &str) -> Result<Search, Unreadable> {
        let why = |e: std::io::Error| Unreadable(format!("{path}: {e}"));
        let file = std::fs::File::open(path).map_err(why)?;
        let len = file.metadata().map_err(why)?.len();
        let r = Image::open(Src::file(&file, len)).map_err(|s| Unreadable(format!("{path}: {s}")))?;
        Search::from_image(path, &r)
    }

    /// The same, from bytes in hand.
    pub fn parse(path: &str, b: &[u8]) -> Result<Search, Unreadable> {
        let r = Image::open(Src::Mem(b)).map_err(|s| Unreadable(format!("{path}: {s}")))?;
        Search::from_image(path, &r)
    }

    fn from_image(path: &str, r: &Image<'_>) -> Result<Search, Unreadable> {
        let why = |s: &str| Unreadable(format!("{path}: {}", r.src.trouble().unwrap_or_else(|| s.to_string())));
        let machine = r.machine().ok_or_else(|| why("truncated header"))?;
        let sections = r.sections().ok_or_else(|| why("no section table could be read"))?;
        let mut out = Search { path: path.to_string(), machine, ..Search::default() };
        for (tag, s) in dynamic_strings(r, &sections) {
            match tag {
                DT_NEEDED => out.needed.push(s),
                DT_RUNPATH => out.runpath = Some(s),
                DT_RPATH => out.rpath = Some(s),
                _ => {}
            }
        }
        match r.src.trouble() {
            Some(t) => Err(Unreadable(format!("{path}: {t}"))),
            None => Ok(out),
        }
    }

    /// The entries that the loader searches for the needs of this object:
    /// those of `DT_RUNPATH`, else those of `DT_RPATH`. glibc ignores the
    /// `DT_RPATH` of an object that has a `DT_RUNPATH`. An empty entry is
    /// left out.
    pub fn dirs(&self) -> Vec<&str> {
        let list = self.runpath.as_deref().or(self.rpath.as_deref()).unwrap_or_default();
        list.split(':').filter(|d| !d.is_empty()).collect()
    }
}
