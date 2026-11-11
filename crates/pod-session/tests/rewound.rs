//! `Rewound::new` (T-172, PodUtils/podssh#186): a router reads the head of
//! a request, then hands the link on with the head first. The reader gets
//! the head again, then the rest of the link; a write goes to the link.

use pod_session::client::Rewound;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[tokio::test]
async fn a_rewound_link_reads_the_head_again_then_the_rest() {
    let (mut far, near) = tokio::io::duplex(1024);
    far.write_all(b"GET / HTTP/1.1\r\nHost: a.loca.lt\r\n\r\nthe body").await.unwrap();
    far.shutdown().await.unwrap();
    // The router reads the head only.
    let mut near = near;
    let mut head = vec![0u8; 35];
    near.read_exact(&mut head).await.unwrap();
    assert!(head.ends_with(b"\r\n\r\n"), "{:?}", String::from_utf8_lossy(&head));
    let mut rewound = Rewound::new(head.clone(), near);
    let mut all = Vec::new();
    rewound.read_to_end(&mut all).await.unwrap();
    assert_eq!(all, b"GET / HTTP/1.1\r\nHost: a.loca.lt\r\n\r\nthe body");
}

#[tokio::test]
async fn a_small_buffer_reads_the_head_in_parts_and_a_write_reaches_the_link() {
    let (mut far, near) = tokio::io::duplex(1024);
    let mut rewound = Rewound::new(b"head".to_vec(), near);
    let mut part = [0u8; 3];
    rewound.read_exact(&mut part).await.unwrap();
    assert_eq!(&part, b"hea");
    far.write_all(b"rest").await.unwrap();
    let mut next = [0u8; 5];
    rewound.read_exact(&mut next).await.unwrap();
    assert_eq!(&next, b"drest");
    rewound.write_all(b"answer").await.unwrap();
    let mut answer = [0u8; 6];
    far.read_exact(&mut answer).await.unwrap();
    assert_eq!(&answer, b"answer");
}

#[tokio::test]
async fn an_empty_head_is_the_link_alone() {
    let (mut far, near) = tokio::io::duplex(64);
    far.write_all(b"only the link").await.unwrap();
    far.shutdown().await.unwrap();
    let mut all = Vec::new();
    Rewound::new(Vec::new(), near).read_to_end(&mut all).await.unwrap();
    assert_eq!(all, b"only the link");
}
