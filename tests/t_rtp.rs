use room_rtc::rtp::{RtpLitePacket, RtpLiteReassembler, RtpLiteSender};

#[test]
fn rtp_lite_packetize_parse_reassemble_roundtrip() {
    let mut sender = RtpLiteSender::new(96, 0x11223344, 1200);
    let mut reasm = RtpLiteReassembler::new();

    // Frame grande para fragmentación
    let original: Vec<u8> = (0..5000u32).map(|i| (i % 256) as u8).collect();
    let ts = 90000u32;

    let packets = sender.packetize_frame(&original, ts);
    assert!(packets.len() > 1, "Debería fragmentar en varios paquetes");

    let mut out = None;
    for raw in packets {
        let pkt = RtpLitePacket::parse(&raw).expect("Paquete inválido");
        let maybe = reasm.push_packet(&pkt);
        if maybe.is_some() {
            out = maybe;
        }
    }

    let reassembled = out.expect("Nunca se reensambló el frame");
    assert_eq!(reassembled, original);
}
