use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

#[derive(Clone)]
pub struct RtcpConfig {
    pub enabled: bool,
    pub interval: Duration,
    pub port_offset: u16,
}

/* ================================================================
 * Reporte de recepción para adaptación de calidad
 * ================================================================ */

#[derive(Clone, Debug, Default)]
pub struct RtpReport {
    pub fraction_lost: f32, // 0.0 .. 1.0
    pub jitter_ms: f32,     // jitter en milisegundos
}

/* ================================================================
 * Estadísticas de recepción RTP (RFC3550)
 * ================================================================ */

#[derive(Debug)]
pub struct RtpRecvStats {
    // Secuencia
    base_seq: u16,
    max_seq: u16,
    received: u32,
    expected_prior: u32,
    received_prior: u32,
    total_lost: i32,

    // Jitter
    last_arrival: Option<Instant>,
    transit_prev: Option<i64>,
    jitter: f64, // en unidades de timestamp (clock)

    // Clock de RTP (video)
    clock_rate: u32,
}

impl Default for RtpRecvStats {
    fn default() -> Self {
        Self {
            base_seq: 0,
            max_seq: 0,
            received: 0,
            expected_prior: 0,
            received_prior: 0,
            total_lost: 0,

            last_arrival: None,
            transit_prev: None,
            jitter: 0.0,

            clock_rate: 90_000, // 90 kHz (video)
        }
    }
}

fn duration_to_f64(d: Duration) -> f64 {
    d.as_secs() as f64 + d.subsec_nanos() as f64 / 1e9
}

impl RtpRecvStats {
    /// Se llama por cada paquete RTP recibido
    pub fn on_rtp_packet(&mut self, _ssrc: u32, seq: u16, rtp_timestamp: u32, arrival: Instant) {
        // Inicializar con el primer paquete
        if self.received == 0 {
            self.base_seq = seq;
            self.max_seq = seq;
        }

        self.received += 1;

        // Pérdidas estilo RFC3550 (simplificado)
        if seq > self.max_seq {
            let diff = seq.wrapping_sub(self.max_seq) as u32;
            if diff > 1 {
                self.total_lost += (diff as i32) - 1;
            }
            self.max_seq = seq;
        }

        // -------------------------
        // JITTER RFC3550
        // -------------------------
        let arrival_prev = self.last_arrival.get_or_insert(arrival);
        let arrival_delta = arrival.duration_since(*arrival_prev);
        *arrival_prev = arrival;

        let delta_seconds = duration_to_f64(arrival_delta);
        let arrival_ts = (delta_seconds * self.clock_rate as f64) as i64;

        let transit = arrival_ts - (rtp_timestamp as i64);

        if let Some(prev_transit) = self.transit_prev {
            let d = (transit - prev_transit).abs();
            // j = j + (|D| - j)/16
            self.jitter += (d as f64 - self.jitter) / 16.0;
        }

        self.transit_prev = Some(transit);
    }

    /// Snapshot usado por RTCP sender y por el TX adaptativo
    pub fn snapshot_report(&self) -> RtpReport {
        // expected = max_seq - base_seq + 1
        let expected = self.max_seq.wrapping_sub(self.base_seq).wrapping_add(1) as u32;

        let _lost = expected as i32 - self.received as i32;

        let expected_interval = expected - self.expected_prior;
        let received_interval = self.received - self.received_prior;
        let lost_interval = expected_interval as i32 - received_interval as i32;

        let fraction_lost = if expected_interval > 0 {
            (lost_interval.max(0) as f32) / expected_interval as f32
        } else {
            0.0
        };

        // jitter en ms: jitter(ts) * 1000 / clock_rate
        let jitter_ms = (self.jitter * 1000.0) / self.clock_rate as f64;

        RtpReport {
            fraction_lost,
            jitter_ms: jitter_ms as f32,
        }
    }
}

/* ================================================================
 * RTCP SENDER (simplificado)
 * ================================================================ */

pub fn spawn_rtcp_sender(
    cfg: RtcpConfig,
    stats: Arc<Mutex<RtpRecvStats>>,
    sock: UdpSocket,
    remote: SocketAddr,
    shutdown: Arc<AtomicBool>,
) -> JoinHandle<()> {
    std::thread::spawn(move || {
        loop {
            // Verificar si se debe cerrar
            if shutdown.load(Ordering::Relaxed) {
                break;
            }

            std::thread::sleep(cfg.interval);

            let rep = {
                if let Ok(s) = stats.lock() {
                    s.snapshot_report()
                } else {
                    continue;
                }
            };

            let payload = format!(
                "RR loss={:.3} jitter_ms={:.1}",
                rep.fraction_lost, rep.jitter_ms
            );

            let _ = sock.send_to(payload.as_bytes(), remote);
        }
    })
}

/* ================================================================
 * RTCP RECEIVER (debug)
 * ================================================================ */

pub fn spawn_rtcp_receiver(
    _cfg: RtcpConfig,
    sock: UdpSocket,
    _quality_tx: std::sync::mpsc::Sender<()>,
    shutdown: Arc<AtomicBool>,
) -> JoinHandle<()> {
    std::thread::spawn(move || {
        let mut buf = [0u8; 1500];

        loop {
            // Verificar si se debe cerrar
            if shutdown.load(Ordering::Relaxed) {
                break;
            }

            match sock.recv_from(&mut buf) {
                Ok((n, src)) => {
                    let msg = String::from_utf8_lossy(&buf[..n]);
                    println!("[RTCP][from {}] {}", src, msg);
                }
                Err(_) => {
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
        }
    })
}
