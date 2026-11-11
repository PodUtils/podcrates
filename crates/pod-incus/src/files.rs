//! The files of an instance (`GET` and `POST
//! /1.0/instances/NAME/files?path=`). A file comes as its bytes with the
//! headers `X-Incus-uid`, `X-Incus-gid`, `X-Incus-mode` (octal) and
//! `X-Incus-type` (`file`, `directory`, `symlink`); a directory comes as a
//! sync answer with the names of its entries; a link as its target. A post
//! gives the same headers, and `X-Incus-write: append` to add to a file.

use pod_proxy::http::Response;

use crate::client::Client;
use crate::envelope;
use crate::error::{Error, Result};
use crate::upload::{Payload, Upload};
use crate::wire::{Body, Request, escape};

/// The kind of a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    File,
    Directory,
    Symlink,
}

impl FileKind {
    pub fn word(self) -> &'static str {
        match self {
            FileKind::File => "file",
            FileKind::Directory => "directory",
            FileKind::Symlink => "symlink",
        }
    }

    fn of(word: &str) -> Option<FileKind> {
        [FileKind::File, FileKind::Directory, FileKind::Symlink].into_iter().find(|k| k.word() == word)
    }
}

/// The owner, the mode and the kind of a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileMeta {
    pub uid: Option<u32>,
    pub gid: Option<u32>,
    /// The permission bits.
    pub mode: Option<u32>,
    pub kind: FileKind,
}

/// What a file holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileContent {
    /// The bytes of a file, or the target of a link.
    Bytes(Vec<u8>),
    /// The names of the entries of a directory.
    Entries(Vec<String>),
}

/// A file of an instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct File {
    pub meta: FileMeta,
    pub content: FileContent,
}

/// A write of a file, a directory or a link.
#[derive(Debug)]
pub struct FileWrite {
    pub kind: FileKind,
    /// The bytes of a file, the target of a link; none for a directory.
    pub content: Payload,
    pub uid: Option<u32>,
    pub gid: Option<u32>,
    pub mode: Option<u32>,
    /// Add to the end of the file in place of a new file.
    pub append: bool,
}

impl FileWrite {
    /// A file of `content`, with the owner and the mode that incus gives.
    pub fn file(content: Payload) -> FileWrite {
        FileWrite { kind: FileKind::File, content, uid: None, gid: None, mode: None, append: false }
    }

    pub fn directory() -> FileWrite {
        FileWrite {
            kind: FileKind::Directory,
            content: Payload::bytes(Vec::new()),
            uid: None,
            gid: None,
            mode: None,
            append: false,
        }
    }

    /// A link to `target`.
    pub fn symlink(target: &str) -> FileWrite {
        let content = Payload::bytes(target.as_bytes().to_vec());
        FileWrite { kind: FileKind::Symlink, content, uid: None, gid: None, mode: None, append: false }
    }
}

fn files_path(name: &str) -> String {
    format!("/1.0/instances/{}/files", escape(name))
}

fn header_number(answer: &Response, name: &str, radix: u32) -> Option<u32> {
    answer.header(name).and_then(|v| u32::from_str_radix(v.trim(), radix).ok())
}

impl Client {
    /// `GET /1.0/instances/NAME/files?path=PATH`.
    pub async fn get_file(&self, name: &str, path: &str) -> Result<File> {
        let files = files_path(name);
        let call = format!("GET {files}");
        let answer = self.raw(&files, &[("path", path)]).await?;
        let word = answer.header("x-incus-type").unwrap_or("file");
        let kind = FileKind::of(word)
            .ok_or_else(|| Error::protocol(&call, format!("a file of type {:?}", crate::error::far_text(word))))?;
        let meta = FileMeta {
            uid: header_number(&answer, "x-incus-uid", 10),
            gid: header_number(&answer, "x-incus-gid", 10),
            mode: header_number(&answer, "x-incus-mode", 8),
            kind,
        };
        let content = if kind == FileKind::Directory {
            let reply = envelope::read(&call, answer.status, &answer.body)?;
            FileContent::Entries(envelope::sync(&call, reply)?)
        } else {
            FileContent::Bytes(answer.body)
        };
        Ok(File { meta, content })
    }

    /// `POST /1.0/instances/NAME/files?path=PATH`.
    pub async fn put_file(&self, name: &str, path: &str, write: FileWrite) -> Result<()> {
        let files = files_path(name);
        let call = format!("POST {files}");
        let mut headers = vec![("X-Incus-type".to_string(), write.kind.word().to_string())];
        if let Some(uid) = write.uid {
            headers.push(("X-Incus-uid".to_string(), uid.to_string()));
        }
        if let Some(gid) = write.gid {
            headers.push(("X-Incus-gid".to_string(), gid.to_string()));
        }
        if let Some(mode) = write.mode {
            headers.push(("X-Incus-mode".to_string(), format!("{:04o}", mode & 0o7777)));
        }
        if write.append {
            headers.push(("X-Incus-write".to_string(), "append".to_string()));
        }
        let body = Body::Stream(Upload::single("application/octet-stream", write.content));
        let request = Request { method: "POST", target: self.target(&files, &[("path", path)]), headers, body };
        self.envelope(&call, request).await.map(drop)
    }
}
