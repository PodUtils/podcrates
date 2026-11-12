//! The public wss:// peers of Yggdrasil (PodUtils/podssh#203): podssh reads
//! the fact `urls`, and picks one peer by a hash of the pair's name, so the
//! order of the fact is a sorted one; each peer is an endpoint of the file.

use pod_services::Service;

fn service() -> &'static Service {
    pod_services::get("yggdrasil-wss-peer").expect("the service yggdrasil-wss-peer")
}

#[test]
fn each_peer_is_a_wss_url_on_443_and_the_list_is_sorted() {
    let urls: Vec<&str> = service().texts("urls").expect("the fact urls").iter().map(|u| u.as_ref()).collect();
    assert!(!urls.is_empty());
    let mut sorted = urls.clone();
    sorted.sort_unstable();
    assert_eq!(urls, sorted, "both ends of a pair read one order");
    for url in &urls {
        let rest = url.strip_prefix("wss://").unwrap_or_else(|| panic!("{url}: not wss://"));
        let authority = rest.split('/').next().unwrap();
        assert!(authority.ends_with(":443"), "{url}: port 443 stated");
    }
}

#[test]
fn each_url_is_an_endpoint_of_the_file() {
    let s = service();
    let urls = s.texts("urls").unwrap();
    assert_eq!(urls.len(), s.endpoints().len(), "one endpoint for each peer");
    for e in s.endpoints() {
        assert_eq!((e.scheme(), e.port()), ("wss", 443), "{}", e.name());
        let path = e.path().unwrap_or("/");
        let url = format!("wss://{}:443{}", e.host(), if path == "/" { "" } else { path });
        assert!(urls.iter().any(|u| u.as_ref() == url), "{url}: in the fact urls");
    }
}
