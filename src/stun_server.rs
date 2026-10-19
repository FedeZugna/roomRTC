use std::io;
use std::net::{SocketAddr, UdpSocket};
use std::sync::Arc;


// Constantes para el protocolo STUN (https://datatracker.ietf.org/doc/html/rfc5389#section-6)
const STUN_MAGIC_COOKIE: u32 = 0x2112A442;
const BINDING_REQUEST: u16 = 0x0001;
const BINDING_RESPONSE: u16 = 0x0101;
const XOR_MAPPED_ADDRESS: u16 = 0x0020;


#[derive(Debug)]
struct StunMessage {
    msg_type: u16,
    msg_length: u16,
    transaction_id: [u8; 12],
    attributes: Vec<u8>,

}

impl StunMessage {

    // Parsear un mensaje STUN desde bytes
    fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 20 {
            return None;
        }

        let msg_type = u16::from_be_bytes([data[0], data[1]]);
        let msg_length = u16::from_be_bytes([data[2], data[3]]);
        let magic_cookie = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);

        if magic_cookie != STUN_MAGIC_COOKIE {
            return None;
        }

        let mut transaction_id = [0u8; 12];
        transaction_id.copy_from_slice(&data[8..20]);

        let attributes = data[20..].to_vec();

        Some(StunMessage {
            msg_type,
            msg_length,
            transaction_id,
            attributes,
        })
    }

    // Crear una Binding Response
    fn create_binding_response(client_addr: SocketAddr, transaction_id: [u8; 12]) -> Vec<u8> {
        let mut response = Vec::new();

        // Tipo de mensaje: Binding Response (0x0101)
        response.extend_from_slice(&BINDING_RESPONSE.to_be_bytes());

        // Longitud del mensaje (se actualizará después)
        let mut msg_length = 0u16;
        response.extend_from_slice(&msg_length.to_be_bytes());

        // Magic Cookie
        response.extend_from_slice(&STUN_MAGIC_COOKIE.to_be_bytes());

        // Transaction ID
        response.extend_from_slice(&transaction_id);

        // Atributo XOR-MAPPED-ADDRESS
        let (ip, port) = match client_addr {
            SocketAddr::V4(addr) => {
                let ip = addr.ip().octets();
                let port = addr.port();
                (ip, port)
            }
            SocketAddr::V6(_) => ([0; 4], 0), // IPv6 lo usamos?
        };

        // Atributo XOR-MAPPED-ADDRESS
        response.extend_from_slice(&XOR_MAPPED_ADDRESS.to_be_bytes()); // Tipo de atributo
        response.extend_from_slice(&8u16.to_be_bytes()); // Longitud (8 bytes para IPv4)
        response.extend_from_slice(&[0x00, 0x01]); // Familia de direcciones (IPv4 = 0x01)

        // XOR del puerto
        let xored_port = port ^ (STUN_MAGIC_COOKIE >> 16) as u16;
        response.extend_from_slice(&xored_port.to_be_bytes());

        // XOR de la IP
        let xored_ip: [u8; 4] = [
            ip[0] ^ ((STUN_MAGIC_COOKIE >> 24) & 0xFF) as u8,
            ip[1] ^ ((STUN_MAGIC_COOKIE >> 16) & 0xFF) as u8,
            ip[2] ^ ((STUN_MAGIC_COOKIE >> 8) & 0xFF) as u8,
            ip[3] ^ (STUN_MAGIC_COOKIE & 0xFF) as u8,
        ];
        response.extend_from_slice(&xored_ip);

        // Actualizar longitud del mensaje
        msg_length = (response.len() - 20) as u16;
        response[2..4].copy_from_slice(&msg_length.to_be_bytes());

        response
    }
}

// Configuración del servidor STUN
#[derive(Clone)]
pub struct StunServerConfig {
    pub bind_addr: String
}


// Estructura principal del servidor STUN
pub struct StunServer {
    config: StunServerConfig,
}

impl StunServer {
    
    pub fn new(config: StunServerConfig) -> Self {
        StunServer { config }
    }

    pub fn start(&self) -> io::Result<()> {
        self.start_udp()
    }

    fn start_udp(&self) -> io::Result<()> {
        let socket = Arc::new(UdpSocket::bind(&self.config.bind_addr)?);
        println!("STUN server (UDP) escuchando en {}", self.config.bind_addr);

        let socket_clone = Arc::clone(&socket);
        let mut buf = [0u8; 1024];
            loop {
                match socket_clone.recv_from(&mut buf) {
                    Ok((amt, src)) => {
                        let data = &buf[..amt];
                        if let Some(stun_msg) = StunMessage::from_bytes(data) {
                            println!("Mensaje STUN recibido: {:?}", stun_msg);
                            println!("Longitud de mensaje STUN recibido: {:?}", stun_msg.msg_length);
                            println!("Atributos de mensaje STUN recibido: {:?}", stun_msg.attributes);
                            
                            if stun_msg.msg_type == BINDING_REQUEST {
                                println!("Recibida Binding Request desde: {}", src);
                                let response = StunMessage::create_binding_response(src, stun_msg.transaction_id);
                                if let Err(e) = socket_clone.send_to(&response, src) {
                                    eprintln!("Error al enviar respuesta: {}", e);
                                } else {
                                    println!("Enviada Binding Response a: {}", src);
                                }
                            }
                        }
                    }
                    Err(e) => eprintln!("Error en recepción UDP: {}", e),
                }
            };

    }

}