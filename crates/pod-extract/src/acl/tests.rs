use super::*;

/// The value of `system.posix_acl_access` that Linux gave for the file of
/// the texts below: `getxattr` of a file whose ACL was set with these ids.
const KERNEL: &str = "0200000001000600ffffffff02000600e8030000020004003930000004000400ffffffff\
                      0800050000000000080001000400000010000600ffffffff20000400ffffffff";

fn hex(b: &[u8]) -> String {
    b.iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn the_text_forms_of_gnu_tar_and_libarchive_give_the_binary_form_of_the_kernel() {
    let forms: [&[u8]; 4] = [
        // GNU tar 1.35 `--acls --numeric-owner`.
        b"user::rw-\nuser:1000:rw-\nuser:12345:r--\ngroup::r--\ngroup:0:r-x\ngroup:4:--x\nmask::rw-\nother::r--",
        // libarchive and star: commas, and the id after a name.
        b"user::rw-,user:ubuntu:rw-:1000,user:12345:r--:12345,group::r--,group:root:r-x:0,group:adm:--x:4,mask::rw-,other::r--",
        // Out of order, short tags, a comment, spaces and a last newline.
        b"o::r--\nm::rw-\n g:4:--x \ng::r--\t#effective:r--\ng:0:xr\nu:12345:r\nu:1000:wr\nu::rw\n",
        // `mask` and `other` with no qualifier field.
        b"user::rw-\nuser:1000:rw-\nuser:12345:r--\ngroup::r--\ngroup:0:r-x\ngroup:4:--x\nmask:rw-\nother:r--\n",
    ];
    for text in forms {
        let got = to_xattr(text).unwrap_or_else(|e| panic!("{}: {e}", String::from_utf8_lossy(text)));
        assert_eq!(hex(&got), KERNEL, "{}", String::from_utf8_lossy(text));
    }
    assert_eq!((attribute(b"access"), attribute(b"default"), attribute(b"ace")), (Some(ACCESS), Some(DEFAULT), None));
}

#[test]
fn an_acl_that_does_not_convert_says_why() {
    let cases: [(&[u8], &str); 12] = [
        (b"user::rw-\nuser:ubuntu:rw-\ngroup::r--\nmask::rw-\nother::r--\n", "names the user \"ubuntu\" with no id"),
        (b"user::rw-\ngroup::r--\ngroup:adm:r-x\nmask::r-x\nother::---", "names the group \"adm\" with no id"),
        (b"user::rw-\nuser:1000:rw-\ngroup::r--\nother::r--", "named entries and no mask"),
        (b"user::rw-\nuser::r--\ngroup::r--\nother::r--", "2 user:: entries"),
        (b"user::rw-\nother::r--", "0 group:: entries"),
        (b"", "0 user:: entries"),
        (b"user::rw-\nuser:5:r--\nuser:5:rw-\ngroup::r--\nmask::rw-\nother::r--", "two entries of the user 5"),
        (b"user::rw-\ngroup::r--\nmask::r--\nmask::rw-\nother::r--", "2 mask:: entries"),
        (b"user::rwz\ngroup::r--\nother::r--", "the permissions"),
        (b"user::rr\ngroup::r--\nother::r--", "the permissions"),
        (b"owner::rw-\ngroup::r--\nother::r--", "the tag"),
        (b"user:4294967295:rw-\nuser::rw-\ngroup::r--\nmask::rw-\nother::r--", "ACL_UNDEFINED_ID"),
    ];
    for (text, why) in cases {
        let e = to_xattr(text).expect_err(&String::from_utf8_lossy(text));
        assert!(e.contains(why), "{}: {e}", String::from_utf8_lossy(text));
    }
    assert!(to_xattr(b"user::\xff\n").unwrap_err().contains("not UTF-8"));
    let many = "user:1:r--\n".repeat(ENTRIES_MAX + 1);
    assert!(to_xattr(many.as_bytes()).unwrap_err().contains("more than"));
}
