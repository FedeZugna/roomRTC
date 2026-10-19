use room_rtc::sdp::SessionDescription;

#[test]
fn sdp_parsing_reads_origin_media_and_attributes() {
    let cfg = r#"v=0
        o=- 123 5 IN IP4 127.0.0.1
        s=demo
        t=0 0
        c=IN IP4 127.0.0.1
        m=video 6000 RTP/AVP 96
        a=source:/dev/video0
        a=framerate:25
        a=rtcp_enabled:true
        a=rtcp_interval_ms:500
        a=rtcp_port_offset:1
        "#;

    let mut sdp = SessionDescription::new();
    sdp = sdp.parse(cfg).expect("SDP parse falló");

    assert_eq!(sdp.session_name, "demo");
    assert_eq!(sdp.origin.unicast_address, "127.0.0.1");
    assert_eq!(sdp.origin.session_id, 123);

    assert!(!sdp.media.is_empty());
    let m = &sdp.media[0];

    assert_eq!(m.media_type, "video");
    assert_eq!(m.port, 6000);

    let mut attrs = std::collections::HashMap::new();
    for a in &m.attributes {
        attrs.insert(a.name.as_str(), a.value.as_deref());
    }

    assert_eq!(attrs.get("source"), Some(&Some("/dev/video0")));
    assert_eq!(attrs.get("framerate"), Some(&Some("25")));
    assert_eq!(attrs.get("rtcp_enabled"), Some(&Some("true")));
    assert_eq!(attrs.get("rtcp_interval_ms"), Some(&Some("500")));
    assert_eq!(attrs.get("rtcp_port_offset"), Some(&Some("1")));
}
