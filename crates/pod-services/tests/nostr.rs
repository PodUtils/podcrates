//! The public Nostr relays (PodUtils/podssh#235): the meeting point of
//! `pod_wormhole::nostr` dials the endpoint `relay` of each service of the
//! kind `nostr-relay`, in the order of the ranks. Both ends of a code read
//! one order.

#[test]
fn each_relay_is_a_wss_endpoint_on_443_in_the_order_of_its_rank() {
    let ids: Vec<&str> = pod_services::of_kind("nostr-relay").map(|s| s.id()).collect();
    assert_eq!(ids, ["primal-nostr", "noslol-nostr"]);
    for s in pod_services::of_kind("nostr-relay") {
        let e = s.endpoint("relay").unwrap_or_else(|| panic!("{}: no endpoint relay", s.id()));
        assert_eq!((e.scheme(), e.port(), e.path()), ("wss", 443, Some("/")), "{}", s.id());
        assert_eq!(s.endpoints().len(), 1, "{}: the endpoint relay alone", s.id());
        assert!(!s.party().is_empty(), "{}: whose it is", s.id());
        for fact in ["message-bytes-most", "subscriptions-most"] {
            assert!(s.int(fact).is_some_and(|n| n > 0), "{}: {fact}", s.id());
        }
    }
}
