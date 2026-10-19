use room_rtc::candidates::{Candidate, CandidateType, Transport};
use std::net::{IpAddr, Ipv4Addr};

fn main() {
    // Creo un candidate de ejemplo (host local en 192.168.0.10:5000)
    let ip = IpAddr::V4(Ipv4Addr::new(192, 168, 0, 10));
    let candidate = Candidate::new(ip, 5000, 1, Transport::Udp, CandidateType::Host);

    println!("Candidate generado:");
    println!("{}", candidate);
}
