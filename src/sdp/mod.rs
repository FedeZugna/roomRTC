// RFC SDP 8866: https://www.rfc-editor.org/rfc/rfc8866

mod errors;
pub use errors::SdpError;

use crate::sdp::parsing::{
    parse_attribute, parse_connection, parse_media, parse_origin, parse_timing,
};

// Import parsing module which contains all the parsers
mod parsing;

pub struct SessionDescription {
    // Protocol Version ("v=") (siempre 0)
    pub version: u32,

    // Origin ("o=")
    pub origin: Origin,

    // Session Name ("s=")
    // MUST be one and only one "s=" line per session description
    // No vacio, usar "-" si no hay nombre o " " (ej: "s=-" o "s= ")
    pub session_name: String,

    // Definir si se necesita "i=" (session information) o "u=" (URI) para mas info
    // Al igual que el "e=" (email) y "p=" (phone) son opcionales

    // Timing ("t=")
    pub timing: Timing,

    // Connection info at session level (optional)
    pub session_connection: Option<ConnectionInfo>,

    // Media Descriptions ("m=")
    pub media: Vec<MediaDescription>,
}

impl Default for SessionDescription {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct Origin {
    pub username: String,
    pub session_id: u64,         // Unique identifier for the session
    pub session_version: u64,    // RECOMMENDED that a timestamp be used ?
    pub network_type: String,    // Usually "IN" for Internet
    pub address_type: String,    // Usually "IP4" or "IP6"
    pub unicast_address: String, // IP address
}

#[derive(Debug, Clone)]
pub struct Timing {
    pub start_time: u64,
    pub stop_time: u64,
}

#[derive(Debug, Clone)]
pub struct MediaDescription {
    pub media_type: String,   // "audio", "video", "application", etc.
    pub port: u16,            // is the transport port to which the media stream is sent.
    pub proto: String,        // is the transport protocol e.x., "UDP/TLS/RTP/SAVPF"
    pub formats: Vec<String>, // is a media format description
    // A session description MUST contain either at least
    // one "c=" (connection_info) line in each media description or a single "c=" line at the session level
    pub connection_info: Option<ConnectionInfo>,
    pub attributes: Vec<Attribute>, // A media description may contain any number of "a=" lines
}

#[derive(Debug, Clone)]
pub struct ConnectionInfo {
    pub network_type: String,       // Normalmente "IN" de internet
    pub address_type: String,       // "IP4" o "IP6"
    pub connection_address: String, // IP addr
}

#[derive(Debug, Clone)]
pub struct Attribute {
    pub name: String,
    pub value: Option<String>,
}

impl SessionDescription {
    // Se crea un nuevo SessionDescription con valores por default
    pub fn new() -> Self {
        SessionDescription {
            version: 0,
            origin: Origin {
                username: "-".to_string(), // Por ahora no se sabe
                session_id: rand::random::<u64>(),
                session_version: 1,
                network_type: "IN".to_string(),
                address_type: "IP4".to_string(),
                unicast_address: "0.0.0.0".to_string(),
            },
            session_name: "-".to_string(),
            timing: Timing {
                start_time: 0,
                stop_time: 0,
            },
            session_connection: None,
            media: Vec::new(),
        }
    }

    pub fn parse(self: SessionDescription, input: &str) -> Result<Self, SdpError> {
        let mut sd = SessionDescription::new();

        let mut current_media: Option<MediaDescription> = None;

        for raw_line in input.lines() {
            let line = raw_line.trim();
            if line.is_empty() {
                continue;
            }

            // Cada línea debe ser "x=valor"
            let (sdp_type, sdp_val) = line
                .split_once('=')
                .ok_or_else(|| SdpError::BadLine(line.to_string()))?;

            // parse con "match" según el tipo
            match sdp_type {
                "v" => {
                    let v: u32 = sdp_val.trim().parse()?;
                    if v != 0 {
                        return Err(SdpError::BadVersion(sdp_val.into()));
                    }
                    sd.version = v;
                }
                "o" => {
                    sd.origin = parse_origin(sdp_val)?;
                }
                "s" => {
                    sd.session_name = sdp_val.to_string();
                }
                "t" => {
                    sd.timing = parse_timing(sdp_val)?;
                }
                "c" => {
                    let conn = parse_connection(sdp_val)?;
                    if let Some(ref mut m) = current_media {
                        m.connection_info = Some(conn);
                    } else {
                        sd.session_connection = Some(conn);
                    }
                }
                "m" => {
                    // cerrar media anterior si había ver
                    if let Some(m) = current_media.take() {
                        sd.media.push(m);
                    }
                    current_media = Some(parse_media(sdp_val)?);
                }
                "a" => {
                    let attr = parse_attribute(sdp_val);
                    if let Some(ref mut m) = current_media {
                        m.attributes.push(attr);
                    }
                }
                // Campos opcionales (ver)
                "i" | "u" | "e" | "p" => {
                    // ver que hacer
                }
                _ => { /* ignorar o error en caso de lineas nulas? */ }
            }
        }

        // push de la última media si quedó abierta
        if let Some(m) = current_media.take() {
            sd.media.push(m);
        }

        // Segun RFC: debe existir un "c=" a nivel sesión o por cada media
        for m in &sd.media {
            if sd.session_connection.is_none() && m.connection_info.is_none() {
                return Err(SdpError::MissingConnectionForMedia(m.media_type.clone()));
            }
        }

        Ok(sd)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_basic_audio_session() {
        let sdp_text = r#"v=0
o=Node 1 1 IN IP4 172.30.8.37
s=Stream1
t=0 0
m=audio 5004 RTP/AVP 96
c=IN IP4 239.30.22.1
a=rtpmap:96 L24/48000/2
a=recvonly
a=ptime:1
a=ts-refclk:ptp=IEEE1588-2008:01-23-45-67-89-AB-CD-EF:127
a=mediaclk:direct=0
a=sync-time:0
"#;

        let sdp_description = SessionDescription::new();

        let sdp = sdp_description
            .parse(sdp_text)
            .expect("failed to parse SDP");

        assert_eq!(sdp.version, 0);
        assert_eq!(sdp.origin.username, "Node");
        assert_eq!(sdp.origin.unicast_address, "172.30.8.37");
        assert_eq!(sdp.session_name, "Stream1");
        assert_eq!(sdp.timing.start_time, 0);
        assert_eq!(sdp.media.len(), 1);

        let media = &sdp.media[0];
        assert_eq!(media.media_type, "audio");
        assert_eq!(media.port, 5004);
        assert_eq!(media.proto, "RTP/AVP");
        assert_eq!(media.formats, vec!["96"]);
        assert!(media.connection_info.is_some());
        assert_eq!(
            media.connection_info.as_ref().unwrap().connection_address,
            "239.30.22.1"
        );

        let attrs: Vec<&str> = media.attributes.iter().map(|a| a.name.as_str()).collect();
        assert!(attrs.contains(&"rtpmap"));
        assert!(attrs.contains(&"recvonly"));
        assert!(attrs.contains(&"ptime"));
        assert!(attrs.contains(&"ts-refclk"));
        assert!(attrs.contains(&"mediaclk"));
        assert!(attrs.contains(&"sync-time"));

        let rtpmap = media
            .attributes
            .iter()
            .find(|a| a.name == "rtpmap")
            .unwrap();
        assert_eq!(rtpmap.value.as_deref(), Some("96 L24/48000/2"));
    }
}
