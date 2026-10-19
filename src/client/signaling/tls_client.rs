//! Cliente TLS para comunicación segura con el signaling server.
//!
//! Este módulo proporciona un cliente TLS que se conecta al signaling server
//! y maneja el envío/recepción de mensajes del protocolo de señalización de forma segura.

use super::Message;
use rustls::{ClientConfig, ClientConnection, RootCertStore, StreamOwned};
use std::fs::File;
use std::io::{BufReader, Read, Write};
use std::net::TcpStream;
use std::sync::Arc;
use std::time::Duration;

/// Cliente TLS para comunicación con el signaling server.
pub struct TlsClient {
    stream: StreamOwned<ClientConnection, TcpStream>,
}

impl TlsClient {
    /// Conecta al signaling server usando TLS.
    ///
    /// # Argumentos
    ///
    /// * `addr` - Dirección del servidor (ej: "127.0.0.1:8443")
    /// * `cert_path` - Ruta al certificado CA para validar el servidor
    /// * `domain` - Nombre de dominio del servidor para validación TLS
    ///
    /// # Errores
    ///
    /// Retorna error si:
    /// - No se puede leer el certificado CA
    /// - Falla la conexión TCP
    /// - Falla el handshake TLS
    pub fn connect(
        addr: &str,
        cert_path: &str,
        domain: &str,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let root_file = File::open(cert_path)?;
        let mut reader = BufReader::new(root_file);
        let mut root_store = RootCertStore::empty();
        for cert in rustls_pemfile::certs(&mut reader) {
            root_store.add(cert?)?;
        }

        let config = ClientConfig::builder()
            .with_root_certificates(root_store)
            .with_no_client_auth();

        println!("[Signaling] Conectando a {}...", addr);
        let sock = TcpStream::connect(addr)?;

        let server_name = domain.to_string().try_into().unwrap();
        let conn = ClientConnection::new(Arc::new(config), server_name)?;
        let stream = StreamOwned::new(conn, sock);

        Ok(Self { stream })
    }

    /// Envía un mensaje al servidor.
    ///
    /// Los mensajes se serializan a JSON y se envían con un prefijo de 4 bytes
    /// indicando la longitud del mensaje.
    ///
    /// # Argumentos
    ///
    /// * `msg` - Mensaje a enviar
    ///
    /// # Errores
    ///
    /// Retorna error si falla la serialización o el envío por la red.
    pub fn send(&mut self, msg: &Message) -> std::io::Result<()> {
        let json_bytes = serde_json::to_vec(msg)?;
        let len = (json_bytes.len() as u32).to_be_bytes();

        self.stream.write_all(&len)?;
        self.stream.write_all(&json_bytes)?;
        self.stream.flush()?;
        Ok(())
    }

    /// Recibe un mensaje del servidor (operación bloqueante).
    ///
    /// Lee primero 4 bytes con la longitud del mensaje, luego lee
    /// esa cantidad de bytes y deserializa el JSON.
    ///
    /// # Errores
    ///
    /// Retorna error si:
    /// - Falla la lectura de la red
    /// - El mensaje está mal formado
    /// - Falla la deserialización del JSON
    pub fn recv(&mut self) -> std::io::Result<Message> {
        let mut len_bytes = [0u8; 4];
        self.stream.read_exact(&mut len_bytes)?;
        let len = u32::from_be_bytes(len_bytes) as usize;

        let mut buffer = vec![0u8; len];
        self.stream.read_exact(&mut buffer)?;

        let msg = serde_json::from_slice(&buffer)?;
        Ok(msg)
    }

    pub fn set_read_timeout(&mut self, timeout: Option<Duration>) -> std::io::Result<()> {
        self.stream.get_ref().set_read_timeout(timeout)
    }

    pub fn try_recv(&mut self) -> std::io::Result<Message> {
        self.recv()
    }
}
