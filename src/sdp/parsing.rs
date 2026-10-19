use crate::sdp::Attribute;
use crate::sdp::ConnectionInfo;
use crate::sdp::MediaDescription;
use crate::sdp::Origin;
use crate::sdp::SdpError;
use crate::sdp::Timing;

pub fn parse_origin(val: &str) -> Result<Origin, SdpError> {
    let mut it = val.split_whitespace();
    let username = it.next().ok_or(SdpError::MissingOrigin)?.to_string();
    let session_id = it.next().ok_or(SdpError::MissingOrigin)?.parse::<u64>()?;
    let session_version = it.next().ok_or(SdpError::MissingOrigin)?.parse::<u64>()?;
    let network_type = it.next().ok_or(SdpError::MissingOrigin)?.to_string();
    let address_type = it.next().ok_or(SdpError::MissingOrigin)?.to_string();
    let unicast_address = it.next().ok_or(SdpError::MissingOrigin)?.to_string();
    Ok(Origin {
        username,
        session_id,
        session_version,
        network_type,
        address_type,
        unicast_address,
    })
}

pub fn parse_timing(val: &str) -> Result<Timing, SdpError> {
    let mut it = val.split_whitespace();
    let start = it.next().ok_or(SdpError::MissingTiming)?.parse::<u64>()?;
    let stop = it.next().ok_or(SdpError::MissingTiming)?.parse::<u64>()?;
    Ok(Timing {
        start_time: start,
        stop_time: stop,
    })
}

pub fn parse_media(val: &str) -> Result<MediaDescription, SdpError> {
    let mut it = val.split_whitespace();
    let media_type = it
        .next()
        .ok_or_else(|| SdpError::BadMedia(val.into()))?
        .to_string();
    let port = it
        .next()
        .ok_or_else(|| SdpError::BadMedia(val.into()))?
        .parse::<u16>()?;
    let proto = it
        .next()
        .ok_or_else(|| SdpError::BadMedia(val.into()))?
        .to_string();
    let formats: Vec<String> = it.map(|s| s.to_string()).collect();
    Ok(MediaDescription {
        media_type,
        port,
        proto,
        formats,
        connection_info: None,
        attributes: Vec::new(),
    })
}

pub fn parse_connection(val: &str) -> Result<ConnectionInfo, SdpError> {
    let mut it = val.split_whitespace();
    let network_type = it
        .next()
        .ok_or_else(|| SdpError::BadConnection(val.into()))?
        .to_string();
    let address_type = it
        .next()
        .ok_or_else(|| SdpError::BadConnection(val.into()))?
        .to_string();
    let connection_address = it
        .next()
        .ok_or_else(|| SdpError::BadConnection(val.into()))?
        .to_string();
    Ok(ConnectionInfo {
        network_type,
        address_type,
        connection_address,
    })
}

pub fn parse_attribute(val: &str) -> Attribute {
    if let Some((name, v)) = val.split_once(':') {
        Attribute {
            name: name.to_string(),
            value: Some(v.to_string()),
        }
    } else {
        Attribute {
            name: val.to_string(),
            value: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---------- Origin ----------
    #[test]
    fn test_parse_origin() {
        let origin_str = "Node 1 1 IN IP4 172.30.8.37";
        let origin = parse_origin(origin_str).unwrap();

        assert_eq!(origin.username, "Node");
        assert_eq!(origin.session_id, 1);
        assert_eq!(origin.session_version, 1);
        assert_eq!(origin.network_type, "IN");
        assert_eq!(origin.address_type, "IP4");
        assert_eq!(origin.unicast_address, "172.30.8.37");
    }

    #[test]
    fn test_parse_origin_missing_field() {
        let origin_str = "Node 1 1 IN IP4"; // Missing unicast-address
        let result = parse_origin(origin_str);
        assert!(result.is_err());
        match result {
            Err(SdpError::MissingOrigin) => {}
            _ => panic!("Expected MissingOrigin error"),
        }
    }

    #[test]
    fn test_parse_origin_bad_number() {
        let origin_str = "Node notanumber 1 IN IP4 172.30.8.37";
        let err = parse_origin(origin_str).unwrap_err();
        match err {
            SdpError::ParseInt(_) => {}
            other => panic!("Expected ParseInt, got {other:?}"),
        }
    }

    // ---------- Timing ----------
    #[test]
    fn test_parse_timing_ok() {
        let t = parse_timing("0 0").unwrap();
        assert_eq!(t.start_time, 0);
        assert_eq!(t.stop_time, 0);

        let t = parse_timing("172345 987654321").unwrap();
        assert_eq!(t.start_time, 172345);
        assert_eq!(t.stop_time, 987_654_321);
    }

    #[test]
    fn test_parse_timing_missing_field() {
        let err = parse_timing("0").unwrap_err();
        match err {
            SdpError::MissingTiming => {}
            other => panic!("Expected MissingTiming, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_timing_bad_number() {
        let err = parse_timing("x y").unwrap_err();
        match err {
            SdpError::ParseInt(_) => {}
            other => panic!("Expected ParseInt, got {other:?}"),
        }
    }

    // ---------- Media ----------
    #[test]
    fn test_parse_media_single_format() {
        let m = parse_media("audio 5004 RTP/AVP 96").unwrap();
        assert_eq!(m.media_type, "audio");
        assert_eq!(m.port, 5004);
        assert_eq!(m.proto, "RTP/AVP");
        assert_eq!(m.formats, vec!["96"]);
        assert!(m.connection_info.is_none());
        assert!(m.attributes.is_empty());
    }

    #[test]
    fn test_parse_media_multiple_formats() {
        let m = parse_media("video 6000 RTP/AVP 96 97 98").unwrap();
        assert_eq!(m.media_type, "video");
        assert_eq!(m.port, 6000);
        assert_eq!(m.proto, "RTP/AVP");
        assert_eq!(m.formats, vec!["96", "97", "98"]);
    }

    #[test]
    fn test_parse_media_bad_port() {
        let err = parse_media("audio notaport RTP/AVP 96").unwrap_err();
        match err {
            SdpError::ParseInt(_) => {}
            other => panic!("Expected ParseInt, got {other:?}"),
        }
    }

    // ---------- Connection ----------
    #[test]
    fn test_parse_connection_ok() {
        let c = parse_connection("IN IP4 239.30.22.1").unwrap();
        assert_eq!(c.network_type, "IN");
        assert_eq!(c.address_type, "IP4");
        assert_eq!(c.connection_address, "239.30.22.1");

        let c6 = parse_connection("IN IP6 ff15::1").unwrap();
        assert_eq!(c6.address_type, "IP6");
        assert_eq!(c6.connection_address, "ff15::1");
    }

    #[test]
    fn test_parse_connection_missing_field() {
        let err = parse_connection("IN IP4").unwrap_err(); // falta address
        match err {
            SdpError::BadConnection(_) => {}
            other => panic!("Expected BadConnection, got {other:?}"),
        }
    }

    // ---------- Attribute ----------
    #[test]
    fn test_parse_attribute_with_value() {
        let a = parse_attribute("rtpmap:96 opus/48000/2");
        assert_eq!(a.name, "rtpmap");
        assert_eq!(a.value.as_deref(), Some("96 opus/48000/2"));
    }

    #[test]
    fn test_parse_attribute_without_value() {
        let a = parse_attribute("recvonly");
        assert_eq!(a.name, "recvonly");
        assert!(a.value.is_none());
    }
}
