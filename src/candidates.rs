//! Módulo candidate
//!
//! Representa un candidato minimo ICE (host / server-reflexive / relayed / peer-reflexive)
//! y calcula la prioridad según la fórmula del texto RFC (5.1.2.1) disponible en el enunciado del ejercicio:
//! priority = (2^24 * type_pref) + (2^8 * local_pref) + (256 - component_id)

use std::fmt;
use std::net::{IpAddr, SocketAddr};

//constantes de tipo (RFC sugiere entre 0 y 126, host > serverReplexive > Relay)
pub const HOST_TYPE_PREF: u32 = 126;
pub const PEERREFLEXIVE_TYPE_PREF: u32 = 110;
pub const SERVERREFLEXIVE_TYPE_PREF: u32 = 100;
pub const RELAY_TYPE_PREF: u32 = 0;

//pub const DEFAULT_COMPONENT_ID :u32 = 1;

//Prioridad base del local preference
pub const DEFAULT_LOCAL_PREF: u32 = 65535;

/// Tipo de transporte para el candidate.
/// De momento sólo documento UDP y TCP (ICE por defecto usa UDP).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    Udp,
    Tcp,
}

impl fmt::Display for Transport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Transport::Udp => write!(f, "UDP"),
            Transport::Tcp => write!(f, "TCP"),
        }
    }
}

/// Tipo de candidato según RFC (host, server-reflexive, peer-reflexive, relayed)
/// RFC (5.1.1)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CandidateType {
    Host,
    ServerReflexive,
    PeerReflexive,
    Relayed,
}

impl fmt::Display for CandidateType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CandidateType::Host => write!(f, "host"),
            CandidateType::ServerReflexive => write!(f, "srflx"),
            CandidateType::PeerReflexive => write!(f, "prflx"),
            CandidateType::Relayed => write!(f, "relay"),
        }
    }
}

/// Representa un candidato ICE.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// Foundation: string identificador.
    pub foundation: String,
    /// Component ID: se usa que 1 = RTP, 2 = RTCP, etc.
    pub component_id: u32,
    /// Transporte
    pub transport: Transport,
    /// Prioridad calculada según RFC
    pub priority: u32,
    /// Dirección base del candidato (IP+puerto)
    pub base_address: SocketAddr,
    /// Tipo (host, srflx, prflx, relay)
    pub candidate_type: CandidateType,
    // Local preference (0..=65535). Cuando solo hay una IP, uso 65535.
    //pub local_preference: u16,
}

impl Candidate {
    /// Crea un nuevo Candidate calculando automatico foundation y priority.
    ///
    pub fn new(
        ip: IpAddr,
        port: u16,
        component_id: u32,
        transport: Transport,
        candidate_type: CandidateType,
    ) -> Self {
        let base_address = SocketAddr::new(ip, port);
        let type_pref = Self::type_preference(&candidate_type);
        let priority = Self::compute_priority(type_pref, DEFAULT_LOCAL_PREF, component_id);
        let foundation = Self::compute_foundation(&candidate_type, &transport, &ip);

        Candidate {
            foundation,
            component_id,
            transport,
            priority,
            base_address,
            candidate_type,
        }
    }

    /// Esta funcion devuelve el valor de preferencia según el tipo
    /// Estos valores son una convención de implementación (el RFC menciona que se permite variar).
    /// El peer-reflexive debe tener preferencia mayor que server-reflexive.
    pub fn type_preference(candidate_type: &CandidateType) -> u32 {
        match candidate_type {
            CandidateType::Host => HOST_TYPE_PREF, // preferencia para host
            CandidateType::PeerReflexive => PEERREFLEXIVE_TYPE_PREF, // mayor a server-reflexive
            CandidateType::ServerReflexive => SERVERREFLEXIVE_TYPE_PREF, // reflexive (STUN)
            CandidateType::Relayed => RELAY_TYPE_PREF, // último recurso (TURN)
        }
    }

    /// Calcula el `priority` usando la fórmula del RFC:
    pub fn compute_priority(type_pref: u32, local_pref: u32, component_id: u32) -> u32 {
        // implemento la fórmula con shifts para mayor claridad
        // la dejo con los numeros en pricipio para probar todo.
        (1 << 24) * type_pref + (1 << 8) * local_pref + (256 - component_id)
    }

    /// Cálculo de foundation (string única por tipo + transporte + dirección base)
    /// Uso: "{type}-{ip}-{port}" truncado si es muy largo.
    fn compute_foundation(
        candidate_type: &CandidateType,
        transport: &Transport,
        ip: &IpAddr,
    ) -> String {
        format!("{:?}-{}-{}", candidate_type, transport, ip)
    }
}

impl fmt::Display for Candidate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "candidate: {} {} {} {} {} typ {:?}",
            self.foundation,
            self.component_id,
            self.transport,
            self.priority,
            self.base_address,
            self.candidate_type,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    #[test]
    fn test_priority_formula_example() {
        // Ejemplo con valores máximos para demostrar cálculo:
        let type_pref = 126u32; // Host prefered
        let local_pref = 65535u32; // mejor local preference
        let component_id = 1u32;

        let prio = Candidate::compute_priority(type_pref, local_pref, component_id);

        // calculado con precisión: 126 * 2^24 + 65535 * 2^8 + (256 - 1) = 2130706431
        assert_eq!(prio, 2130706431u32);
    }

    #[test]
    fn test_host_candidate() {
        let ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 2));
        let candidate = Candidate::new(ip, 5000, 1, Transport::Udp, CandidateType::Host);

        assert_eq!(candidate.component_id, 1);
        assert_eq!(candidate.transport, Transport::Udp);
        assert_eq!(candidate.candidate_type, CandidateType::Host);
        assert_eq!(candidate.base_address, SocketAddr::new(ip, 5000));
        assert!(candidate.priority > 0);

        println!("{}", candidate); // Debug del formato SDP
    }

    #[test]
    fn test_display_format() {
        let ip = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1));
        let candidate = Candidate::new(ip, 3478, 1, Transport::Udp, CandidateType::ServerReflexive);

        let output = format!("{}", candidate);

        println!("Actual Candidate Output: '{}'", output);
        assert!(output.contains("UDP"));
        assert!(output.contains("3478"));
        assert!(output.contains("ServerReflexive"));
    }
}
