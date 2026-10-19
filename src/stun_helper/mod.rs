use std::io::{Error, ErrorKind};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, ToSocketAddrs, UdpSocket};
use std::time::Duration;

const STUN_MAGIC_COOKIE: u32 = 0x2112A442;
const BINDING_REQUEST: u16 = 0x0001;
const BINDING_RESPONSE: u16 = 0x0101;
const ATTR_XOR_MAPPED_ADDRESS: u16 = 0x0020;
const ATTR_MAPPED_ADDRESS: u16 = 0x0001;
const STUN_SERVER_DEFAULT: &str = "stun.l.google.com:19302";

#[derive(Debug, Clone)]
pub struct PublicAddress {
    pub ip: IpAddr,
    pub port: u16,
}

pub fn discover_public_address(_local_port: u16) -> Result<PublicAddress, Error> {
    let server_addr = resolve_stun_server(STUN_SERVER_DEFAULT)?;
    let local_bind = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 0);

    let socket = UdpSocket::bind(local_bind)?;
    socket.set_read_timeout(Some(Duration::from_secs(5)))?;
    socket.set_write_timeout(Some(Duration::from_secs(5)))?;

    let transaction_id = generate_transaction_id();
    let request = build_binding_request(&transaction_id);

    socket.send_to(&request, server_addr)?;

    let mut buf = vec![0u8; 1500];
    let (len, _) = socket.recv_from(&mut buf)?;

    parse_binding_response(&buf[..len], &transaction_id)
}

fn resolve_stun_server(server: &str) -> Result<SocketAddr, Error> {
    if let Ok(addr) = server.parse::<SocketAddr>()
        && addr.is_ipv4()
    {
        return Ok(addr);
    }

    // Esto es para el caso de que queramos usar el de cloudflare "stun.cloudflare.com"
    let server_with_port = if !server.contains(':') {
        format!("{}:3478", server)
    } else {
        server.to_string()
    };

    let addrs: Vec<SocketAddr> = server_with_port.to_socket_addrs()?.collect();

    for addr in addrs {
        if addr.is_ipv4() {
            return Ok(addr);
        }
    }

    Err(Error::new(
        ErrorKind::InvalidInput,
        "No se encontro direccion IPv4 para el servidor STUN",
    ))
}

fn generate_transaction_id() -> [u8; 12] {
    let mut id = [0u8; 12];
    use rand::Rng;
    let mut rng = rand::rng();
    rng.fill(&mut id);
    id
}

fn build_binding_request(transaction_id: &[u8; 12]) -> Vec<u8> {
    let mut packet = Vec::new();

    packet.extend_from_slice(&BINDING_REQUEST.to_be_bytes());
    packet.extend_from_slice(&0u16.to_be_bytes());
    packet.extend_from_slice(&STUN_MAGIC_COOKIE.to_be_bytes());
    packet.extend_from_slice(transaction_id);

    let length = (packet.len() - 20) as u16;
    packet[2..4].copy_from_slice(&length.to_be_bytes());

    packet
}

fn parse_binding_response(
    data: &[u8],
    expected_transaction_id: &[u8; 12],
) -> Result<PublicAddress, Error> {
    if data.len() < 20 {
        return Err(Error::new(
            ErrorKind::InvalidData,
            "Respuesta STUN muy corta",
        ));
    }

    let msg_type = u16::from_be_bytes([data[0], data[1]]);
    if msg_type != BINDING_RESPONSE {
        return Err(Error::new(ErrorKind::InvalidData, "No es Binding Response"));
    }

    let magic = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
    if magic != STUN_MAGIC_COOKIE {
        return Err(Error::new(ErrorKind::InvalidData, "Magic cookie invalido"));
    }

    let recv_transaction_id = &data[8..20];
    if recv_transaction_id != expected_transaction_id {
        return Err(Error::new(
            ErrorKind::InvalidData,
            "Transaction ID no coincide",
        ));
    }

    let mut offset = 20;
    while offset + 4 <= data.len() {
        let attr_type = u16::from_be_bytes([data[offset], data[offset + 1]]);
        let attr_len = u16::from_be_bytes([data[offset + 2], data[offset + 3]]) as usize;
        offset += 4;

        if offset + attr_len > data.len() {
            break;
        }

        if attr_type == ATTR_XOR_MAPPED_ADDRESS {
            return parse_xor_mapped_address(&data[offset..offset + attr_len]);
        } else if attr_type == ATTR_MAPPED_ADDRESS {
            return parse_mapped_address(&data[offset..offset + attr_len]);
        }

        offset += attr_len;
        offset = (offset + 3) & !3;
    }

    Err(Error::new(
        ErrorKind::NotFound,
        "No se encontro direccion mapeada",
    ))
}

fn parse_xor_mapped_address(data: &[u8]) -> Result<PublicAddress, Error> {
    if data.len() < 8 {
        return Err(Error::new(
            ErrorKind::InvalidData,
            "XOR-MAPPED-ADDRESS muy corto",
        ));
    }

    let family = data[1];
    let xor_port = u16::from_be_bytes([data[2], data[3]]);
    let port = xor_port ^ (STUN_MAGIC_COOKIE >> 16) as u16;

    let ip = if family == 0x01 {
        let xor_ip = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
        let ip_u32 = xor_ip ^ STUN_MAGIC_COOKIE;
        IpAddr::from(ip_u32.to_be_bytes())
    } else {
        return Err(Error::new(ErrorKind::Unsupported, "IPv6 no soportado"));
    };

    Ok(PublicAddress { ip, port })
}

fn parse_mapped_address(data: &[u8]) -> Result<PublicAddress, Error> {
    if data.len() < 8 {
        return Err(Error::new(
            ErrorKind::InvalidData,
            "MAPPED-ADDRESS muy corto",
        ));
    }

    let family = data[1];
    let port = u16::from_be_bytes([data[2], data[3]]);

    let ip = if family == 0x01 {
        IpAddr::from([data[4], data[5], data[6], data[7]])
    } else {
        return Err(Error::new(ErrorKind::Unsupported, "IPv6 no soportado"));
    };

    Ok(PublicAddress { ip, port })
}
