//! The public MQTT brokers (PodUtils/podssh#205): the meeting point of
//! `pod_wormhole::mqtt` dials the endpoint `broker` of each service of the
//! kind `mqtt-broker`, in the order of the ranks, with the login of the
//! facts `user` and `password` where a service has them. Both ends of a
//! code read one order.

#[test]
fn each_broker_is_a_wss_endpoint_on_443_in_the_order_of_its_rank() {
    let ids: Vec<&str> = pod_services::of_kind("mqtt-broker").map(|s| s.id()).collect();
    assert_eq!(ids, ["shiftr-mqtt", "eclipse-mqtt"]);
    for s in pod_services::of_kind("mqtt-broker") {
        let e = s.endpoint("broker").unwrap_or_else(|| panic!("{}: no endpoint broker", s.id()));
        assert_eq!((e.scheme(), e.port()), ("wss", 443), "{}", s.id());
        assert!(e.path().is_some_and(|p| p.starts_with('/')), "{}: a path", s.id());
        assert_eq!(s.text("user").is_some(), s.text("password").is_some(), "{}: a login is whole or none", s.id());
        for other in s.endpoints().iter().filter(|e| e.name() != "broker") {
            assert_eq!(other.role(), Some("document"), "{}: {} is a document", s.id(), other.name());
        }
    }
}

#[test]
fn the_public_instance_of_shiftr_has_its_public_login() {
    let s = pod_services::get("shiftr-mqtt").expect("the service shiftr-mqtt");
    assert_eq!((s.text("user"), s.text("password")), (Some("public"), Some("public")));
    assert_eq!(s.int("payload-bytes-most"), Some(64_000));
    assert_eq!(pod_services::get("eclipse-mqtt").and_then(|s| s.text("user")), None, "no login");
}
