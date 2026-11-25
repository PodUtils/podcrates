//! The fields of a text of `/proc/PID/stat` (`proc(5)`), for each reader
//! of a crate.

/// The fields after the command name of a text of `/proc/PID/stat`, field 3
/// (the state) first: field `n` of `proc(5)` is at the index `n - 3`. The
/// name is up to the last `)`: it can hold a space and a `)`. The count of
/// the fields is not checked: qemu-user and a kernel before 3.5 write 44
/// fields, a new kernel 52. `None` for a text with no `)`.
pub fn after_name(stat: &str) -> Option<Vec<&str>> {
    let (_, rest) = stat.rsplit_once(')')?;
    Some(rest.split_whitespace().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The text of `open_self_stat` of qemu-user (`linux-user/syscall.c`):
    /// 44 fields, 0 in most of them.
    const QEMU: &str = "4242 (pod_enter-0f1e2d) R 4241 4242 0 0 0 0 0 0 0 0 0 0 0 0 0 0 3 0 1234567 \
        0 0 0 0 0 -69632 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0\n";

    #[test]
    fn the_fields_start_after_the_last_parenthesis_of_the_name() {
        let kernel = "1 (systemd) S 0 1 1 0 -1 4194560 52 0 0 0 1 2 0 0 20 0 1 0 9 \
            23 2 18446744073709551615 1 1 0 0 0 0 671173123 4096 1260 0 0 0 17 0 0 0 0 0 0 0 0 0 0 0 0 0 0\n";
        let f = after_name(kernel).expect("a kernel text");
        assert_eq!((f.len(), f[0], f[1], f[4]), (50, "S", "0", "0"), "fields 3, 4 and 7");
        let f = after_name("12 (a b) c) S 9 12 12 34816").expect("a name with a space and a parenthesis");
        assert_eq!(f, ["S", "9", "12", "12", "34816"]);
        let f = after_name(QEMU).expect("the text of qemu-user");
        assert_eq!((f.len(), f[0], f[1], f[17]), (42, "R", "4241", "3"), "44 fields; field 20 is the threads");
        assert_eq!(after_name("12 (x)"), Some(vec![]));
        assert_eq!(after_name("no parenthesis"), None);
    }
}
