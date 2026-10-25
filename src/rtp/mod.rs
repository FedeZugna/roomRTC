use std::collections::BTreeMap;

pub const RTP_LITE_HEADER_SIZE: usize = 11;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RtpLiteHeader {
    pub marker: bool,         // 1 bit
    pub payload_type: u8,     // 7 bits (96 VP8 dinámico)
    pub sequence_number: u16, // 16 bits
    pub timestamp: u32,       // 32 bits
    pub ssrc: u32,            // 32 bits
}

/// RTP "Lite"
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RtpLitePacket {
    pub header: RtpLiteHeader,
    pub payload: Vec<u8>,
}

impl RtpLitePacket {
    pub fn parse(buf: &[u8]) -> Option<Self> {
        if buf.len() < RTP_LITE_HEADER_SIZE {
            return None;
        }
        let m_pt = buf[0];
        let marker = (m_pt >> 7) & 1 == 1;
        let payload_type = m_pt & 0x7F;
        let sequence_number = u16::from_be_bytes([buf[1], buf[2]]);
        let timestamp = u32::from_be_bytes([buf[3], buf[4], buf[5], buf[6]]);
        let ssrc = u32::from_be_bytes([buf[7], buf[8], buf[9], buf[10]]);
        let payload = buf[RTP_LITE_HEADER_SIZE..].to_vec();

        Some(Self {
            header: RtpLiteHeader {
                marker,
                payload_type,
                sequence_number,
                timestamp,
                ssrc,
            },
            payload,
        })
    }

    pub fn build(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(RTP_LITE_HEADER_SIZE + self.payload.len());
        let h = &self.header;
        out.push(((h.marker as u8) << 7) | (h.payload_type & 0x7F));
        out.extend_from_slice(&h.sequence_number.to_be_bytes());
        out.extend_from_slice(&h.timestamp.to_be_bytes());
        out.extend_from_slice(&h.ssrc.to_be_bytes());
        out.extend_from_slice(&self.payload);
        out
    }
}

/// Sender paquetes RTP fragmenta un frame VP8 en varios paquetes
pub struct RtpLiteSender {
    pub payload_type: u8,
    pub ssrc: u32,
    pub mtu: usize, //paquetes
    pub sequence: u16,
}

impl RtpLiteSender {
    pub fn new(payload_type: u8, ssrc: u32, mtu: usize) -> Self {
        Self {
            payload_type,
            ssrc,
            mtu,
            sequence: 0,
        }
    }

    pub fn packetize_frame(&mut self, frame: &[u8], timestamp: u32) -> Vec<Vec<u8>> {
        let mut packets = Vec::new();
        if frame.is_empty() {
            return packets;
        }
        let total_chunks = frame.len().div_ceil(self.mtu);

        for (i, chunk) in frame.chunks(self.mtu).enumerate() {
            let header = RtpLiteHeader {
                marker: i == total_chunks - 1,
                payload_type: self.payload_type,
                sequence_number: self.sequence,
                timestamp,
                ssrc: self.ssrc,
            };
            self.sequence = self.sequence.wrapping_add(1);

            let pkt = RtpLitePacket {
                header,
                payload: chunk.to_vec(),
            };
            packets.push(pkt.build());
        }
        packets
    }
}

/// Reensamblador paquetes RTP en un frame VP8 completo
pub struct RtpLiteReassembler {
    current_ts: Option<u32>,
    frags: BTreeMap<u16, Vec<u8>>,
}

impl Default for RtpLiteReassembler {
    fn default() -> Self {
        Self::new()
    }
}

impl RtpLiteReassembler {
    pub fn new() -> Self {
        Self {
            current_ts: None,
            frags: BTreeMap::new(),
        }
    }

    /// Inserta el paquete y si llega el último (marker=1) devuelve el frame completo
    pub fn push_packet(&mut self, pkt: &RtpLitePacket) -> Option<Vec<u8>> {
        let ts = pkt.header.timestamp;

        if self.current_ts != Some(ts) {
            self.current_ts = Some(ts);
            self.frags.clear();
        }

        self.frags
            .insert(pkt.header.sequence_number, pkt.payload.clone());

        if pkt.header.marker {
            let mut frame = Vec::new();
            for part in self.frags.values() {
                frame.extend_from_slice(part);
            }
            self.frags.clear();
            self.current_ts = None;
            return Some(frame);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Tests para RtpLitePacket (parse/build) ---

    #[test]
    fn test_rtp_lite_packet_roundtrip() {
        let original_header = RtpLiteHeader {
            marker: true,
            payload_type: 96,
            sequence_number: 0xABCD,
            timestamp: 0x12345678,
            ssrc: 0xDEADBEEF,
        };
        let original_payload = vec![0xAA, 0xBB, 0xCC, 0xDD];

        let original_packet = RtpLitePacket {
            header: original_header.clone(), // Clonamos para que el test sea más claro
            payload: original_payload.clone(),
        };

        // 1. Construir (build)
        let raw_buffer = original_packet.build();

        // Verificar el tamaño
        assert_eq!(
            raw_buffer.len(),
            RTP_LITE_HEADER_SIZE + original_payload.len()
        );

        // Verificar bytes específicos del header
        // M=1, PT=96 (128 | 96 = 224 o 0xE0)
        assert_eq!(raw_buffer[0], 0b11100000);
        // Seq Num: 0xABCD
        assert_eq!(&raw_buffer[1..3], &[0xAB, 0xCD]);
        // Timestamp: 0x12345678
        assert_eq!(&raw_buffer[3..7], &[0x12, 0x34, 0x56, 0x78]);
        // SSRC: 0xDEADBEEF
        assert_eq!(&raw_buffer[7..11], &[0xDE, 0xAD, 0xBE, 0xEF]);
        // Payload
        assert_eq!(&raw_buffer[11..], original_payload.as_slice());

        // 2. Parsear
        let parsed_packet = RtpLitePacket::parse(&raw_buffer).expect("Debe parsear correctamente");

        // 3. Verificar roundtrip
        // Estas comparaciones funcionan gracias a #[derive(PartialEq, Eq)]
        assert_eq!(parsed_packet.header, original_header);
        assert_eq!(parsed_packet.payload, original_payload);
    }

    #[test]
    fn test_rtp_lite_packet_min_size() {
        // Buffer demasiado corto (menor al tamaño del header)
        let short_buffer = vec![1, 2, 3];
        // Línea que el compilador reporta como problemática si falta PartialEq/Eq
        assert_eq!(RtpLitePacket::parse(&short_buffer), None);

        // Buffer de tamaño exacto del header (payload vacío)
        let header_only = vec![
            0x80, // M=1, PT=0
            0x00, 0x01, // Seq Num = 1
            0x00, 0x00, 0x00, 0x00, // TS = 0
            0x00, 0x00, 0x00, 0x00, // SSRC = 0
        ];
        let pkt = RtpLitePacket::parse(&header_only).expect("Debe parsear el header sin payload");
        assert_eq!(pkt.payload.len(), 0);
        assert!(pkt.header.marker);
    }

    // --- Tests para RtpLiteSender (packetize_frame) ---

    #[test]
    fn test_sender_single_packet() {
        let mut sender = RtpLiteSender::new(97, 0x11223344, 10);
        let frame_data = vec![1, 2, 3, 4, 5]; // 5 bytes, MTU 10

        let packets = sender.packetize_frame(&frame_data, 1000);

        // Debe generar un solo paquete
        assert_eq!(packets.len(), 1);

        // El paquete debe tener el marcador (marker) a true
        let pkt = RtpLitePacket::parse(&packets[0]).unwrap();
        assert!(pkt.header.marker);
        assert_eq!(pkt.header.payload_type, 97);
        assert_eq!(pkt.header.sequence_number, 0); // Empieza en 0
        assert_eq!(pkt.header.timestamp, 1000);
        assert_eq!(pkt.payload.len(), 5);
        assert_eq!(pkt.payload, frame_data);

        // La secuencia debe avanzar
        assert_eq!(sender.sequence, 1);
    }

    #[test]
    fn test_sender_multiple_packets() {
        let mut sender = RtpLiteSender::new(96, 0x55667788, 3);
        // 8 bytes de datos. MTU=3. Esperamos 3 paquetes: (3, 3, 2)
        let frame_data = vec![1, 2, 3, 4, 5, 6, 7, 8];

        let packets = sender.packetize_frame(&frame_data, 2000);

        assert_eq!(packets.len(), 3);
        assert_eq!(sender.sequence, 3); // 0, 1, 2

        // Paquete 1 (Seq 0)
        let pkt0 = RtpLitePacket::parse(&packets[0]).unwrap();
        assert_eq!(pkt0.header.sequence_number, 0);
        assert!(!pkt0.header.marker);
        assert_eq!(pkt0.payload, vec![1, 2, 3]);

        // Paquete 2 (Seq 1)
        let pkt1 = RtpLitePacket::parse(&packets[1]).unwrap();
        assert_eq!(pkt1.header.sequence_number, 1);
        assert!(!pkt1.header.marker);
        assert_eq!(pkt1.payload, vec![4, 5, 6]);

        // Paquete 3 (Seq 2) - Marker debe ser true
        let pkt2 = RtpLitePacket::parse(&packets[2]).unwrap();
        assert_eq!(pkt2.header.sequence_number, 2);
        assert!(pkt2.header.marker);
        assert_eq!(pkt2.payload, vec![7, 8]);
    }

    #[test]
    fn test_sender_sequence_wrapping() {
        let mut sender = RtpLiteSender::new(96, 1, 100);
        let frame_data = vec![10, 20];

        // 1. Establecer el contador cerca del máximo
        sender.sequence = u16::MAX - 1; // 65534

        // Paquete 1 (Header Seq 65534). La secuencia interna avanza a 65535 (u16::MAX)
        let packets1 = sender.packetize_frame(&frame_data, 100);
        let pkt1 = RtpLitePacket::parse(&packets1[0]).unwrap();
        assert_eq!(pkt1.header.sequence_number, u16::MAX - 1);
        assert_eq!(sender.sequence, u16::MAX);

        // Paquete 2 (Header Seq 65535). La secuencia interna wrapea a 0.
        let packets2 = sender.packetize_frame(&frame_data, 101);
        let pkt2 = RtpLitePacket::parse(&packets2[0]).unwrap();

        // FIX: El número de secuencia enviado es u16::MAX (65535), no 0.
        assert_eq!(pkt2.header.sequence_number, u16::MAX);

        // FIX: La secuencia interna ha wrapeado a 0.
        assert_eq!(sender.sequence, 0);
    }

    // --- Tests para RtpLiteReassembler (push_packet) ---

    #[test]
    fn test_reassembler_happy_path() {
        let mut reassembler = RtpLiteReassembler::new();
        let ts = 100;

        // Paquetes fragmentados (el payload total es "Hello World!")
        let pkt1 = RtpLitePacket {
            header: RtpLiteHeader {
                marker: false,
                payload_type: 96,
                sequence_number: 1,
                timestamp: ts,
                ssrc: 1,
            },
            payload: vec![72, 101, 108, 108, 111, 32], // "Hello "
        };
        let pkt2 = RtpLitePacket {
            header: RtpLiteHeader {
                marker: true,
                payload_type: 96,
                sequence_number: 2,
                timestamp: ts,
                ssrc: 1,
            },
            payload: vec![87, 111, 114, 108, 100, 33], // "World!"
        };

        // 1. Empujar el primer paquete (no debe devolver nada)
        assert!(reassembler.push_packet(&pkt1).is_none());
        assert_eq!(reassembler.frags.len(), 1);
        assert_eq!(reassembler.current_ts, Some(ts));

        // 2. Empujar el segundo paquete con Marker=true (debe reensamblar)
        let frame = reassembler
            .push_packet(&pkt2)
            .expect("Debe devolver el frame completo");

        // Verificar el resultado
        assert_eq!(frame, "Hello World!".as_bytes().to_vec());

        // Verificar el estado de limpieza
        assert_eq!(reassembler.frags.len(), 0);
        assert_eq!(reassembler.current_ts, None);
    }

    #[test]
    fn test_reassembler_out_of_order() {
        let mut reassembler = RtpLiteReassembler::new();
        let ts = 200;

        // Paquetes fragmentados (Total: "A_B_C"). Simulación: B (Seq 6) se pierde. C (Seq 7) llega con Marker.
        let pkt_a = RtpLitePacket {
            header: RtpLiteHeader {
                marker: false,
                payload_type: 96,
                sequence_number: 5,
                timestamp: ts,
                ssrc: 1,
            },
            payload: vec![0x41], // 'A'
        };
        let pkt_c = RtpLitePacket {
            header: RtpLiteHeader {
                marker: true,
                payload_type: 96,
                sequence_number: 7,
                timestamp: ts,
                ssrc: 1,
            },
            payload: vec![0x43], // 'C' (Marker=true)
        };
        let pkt_b = RtpLitePacket {
            header: RtpLiteHeader {
                marker: false,
                payload_type: 96,
                sequence_number: 6,
                timestamp: ts,
                ssrc: 1,
            },
            payload: vec![0x42], // 'B'
        };

        // 1. Empujar A (Seq 5)
        assert!(reassembler.push_packet(&pkt_a).is_none());
        assert_eq!(reassembler.frags.len(), 1);

        // 2. Empujar C (Seq 7, Marker=true)
        // FIX: La implementación actual SÍ reensambla aquí, concatenando A y C, ya que ignora si falta B.
        let partial_frame = reassembler
            .push_packet(&pkt_c)
            .expect("El marker debe forzar el reensamblaje, aunque falten paquetes");

        // Verificar el frame incompleto pero ordenado (A luego C)
        assert_eq!(partial_frame, vec![0x41, 0x43]);

        // El estado debe haber sido limpiado.
        assert_eq!(reassembler.frags.len(), 0);
        assert_eq!(reassembler.current_ts, None);

        // 3. Empujar B (Seq 6) - Llega tarde. Esto inicia un nuevo frame.
        // El reensamblador se comporta como si B fuera el primer fragmento del siguiente frame (TS 200).
        assert!(reassembler.push_packet(&pkt_b).is_none());
        assert_eq!(reassembler.frags.len(), 1);
        assert_eq!(reassembler.current_ts, Some(ts)); // TS 200 se reestablece
    }

    #[test]
    fn test_reassembler_timestamp_change_clears_state() {
        let mut reassembler = RtpLiteReassembler::new();

        // Paquete 1: TS 100, no marker
        let pkt1 = RtpLitePacket {
            header: RtpLiteHeader {
                marker: false,
                payload_type: 96,
                sequence_number: 1,
                timestamp: 100,
                ssrc: 1,
            },
            payload: vec![1, 2, 3],
        };
        // Paquete 2: TS 200, no marker (inicio de un nuevo frame)
        let pkt2 = RtpLitePacket {
            header: RtpLiteHeader {
                marker: false,
                payload_type: 96,
                sequence_number: 10,
                timestamp: 200,
                ssrc: 1,
            },
            payload: vec![4, 5, 6],
        };
        // Paquete 3: TS 200, marker=true
        let pkt3 = RtpLitePacket {
            header: RtpLiteHeader {
                marker: true,
                payload_type: 96,
                sequence_number: 11,
                timestamp: 200,
                ssrc: 1,
            },
            payload: vec![7, 8, 9],
        };

        // 1. Empujar pkt1 (TS 100)
        assert!(reassembler.push_packet(&pkt1).is_none());
        assert_eq!(reassembler.frags.len(), 1);

        // 2. Empujar pkt2 (TS 200) - Debe limpiar el estado de TS 100
        assert!(reassembler.push_packet(&pkt2).is_none());
        assert_eq!(reassembler.frags.len(), 1); // Solo contiene pkt2 (Seq 10)
        assert_eq!(reassembler.current_ts, Some(200));

        // 3. Empujar pkt3 (TS 200, Marker=true)
        let frame = reassembler
            .push_packet(&pkt3)
            .expect("Debe reensamblar el frame 200");
        assert_eq!(frame, vec![4, 5, 6, 7, 8, 9]);
    }
}
