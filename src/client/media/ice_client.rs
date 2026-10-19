//! Cliente ICE Lite para establecer conectividad peer-to-peer.

use crate::client::types::ClientResult;
use crate::stun_helper;
use std::io::{Error, ErrorKind};
use std::net::{IpAddr, SocketAddr, UdpSocket};

/// Cliente ICE Lite para comunicación RTP/RTCP.
///
/// Maneja la configuración de sockets UDP y parámetros para transmisión de medios.
pub struct ClienteICELite {
    /// Socket UDP para recepción de datos.
    pub socket_rx: UdpSocket,
    /// Dirección local (privada) del cliente.
    pub direccion_local: SocketAddr,
    /// Dirección remota del peer.
    pub direccion_remota: SocketAddr,
    /// Dirección pública descubierta mediante STUN (si está disponible).
    pub direccion_publica: Option<SocketAddr>,
    /// Flag que indica si RTCP está habilitado.
    pub rtcp_enabled: bool,
    /// Intervalo en milisegundos para envío de reportes RTCP.
    pub rtcp_interval_ms: u64,
    /// Offset del puerto para RTCP (por defecto +1 del puerto RTP).
    pub rtcp_port_offset: u16,
}

/// Función auxiliar para parsear dirección IP y puerto.
///
/// # Argumentos
/// * `ip` - String con la dirección IP.
/// * `port` - Número de puerto.
///
/// # Retorna
/// * `Ok(SocketAddr)` - Dirección socket parseada exitosamente.
/// * `Err` - Error si el formato de IP es inválido.
fn addr(ip: &str, port: u16) -> ClientResult<SocketAddr> {
    let ip: IpAddr = ip.parse()?;
    Ok(SocketAddr::new(ip, port))
}

impl ClienteICELite {
    /// Crea una nueva instancia de ClienteICELite.
    ///
    /// Inicializa el socket UDP, descubre la dirección pública mediante STUN,
    /// y configura los parámetros por defecto para RTCP.
    ///
    /// # Argumentos
    /// * `ip_local` - Dirección IP local para bind.
    /// * `puerto_local` - Puerto local para bind.
    /// * `ip_remota` - Dirección IP del peer remoto.
    /// * `puerto_remoto` - Puerto del peer remoto.
    ///
    /// # Retorna
    /// * `Ok(ClienteICELite)` - Cliente creado exitosamente.
    /// * `Err(Error)` - Error de I/O al crear socket o parsear direcciones.
    pub fn new(
        ip_local: &str,
        puerto_local: u16,
        ip_remota: &str,
        puerto_remoto: u16,
    ) -> Result<Self, Error> {
        let direccion_local = addr(ip_local, puerto_local)
            .map_err(|e| Error::new(ErrorKind::InvalidInput, e.to_string()))?;
        let direccion_remota = addr(ip_remota, puerto_remoto)
            .map_err(|e| Error::new(ErrorKind::InvalidInput, e.to_string()))?;
        let socket_rx = UdpSocket::bind((ip_local, puerto_local))?;
        socket_rx.set_nonblocking(true)?;

        let direccion_publica = match stun_helper::discover_public_address(puerto_local) {
            Ok(public) => Some(SocketAddr::new(public.ip, public.port)),
            Err(_) => None,
        };

        Ok(Self {
            socket_rx,
            direccion_local,
            direccion_remota,
            direccion_publica,
            rtcp_enabled: false,
            rtcp_interval_ms: 1000,
            rtcp_port_offset: 1,
        })
    }
}
