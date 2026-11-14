//! pod-udf: the search for the anchor in an image of zeros, so that the
//! reader links in.

pub fn udf() -> String {
    let zeros = vec![0u8; 1 << 20];
    let found = pod_udf::find(zeros.as_slice(), "/sources/boot.wim");
    format!("pod-udf: {}", found.map_or_else(|e| e.to_string(), |x| format!("{} bytes", x.size)))
}
