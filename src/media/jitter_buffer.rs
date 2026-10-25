use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::time::{Duration, Instant};

/// Frame VP8 recibido desde RTP
/// Este tipo coincide con `EncodedFrame` usado por tu decoder actual.
#[derive(Debug, Clone)]
pub struct JitterFrame {
    pub timestamp_90khz: u32, // timestamp RTP de 90 kHz
    pub arrival: Instant,     // cuándo llegó
    pub data: Vec<u8>,        // payload VP8 completa
}

/// Configuración para el jitter buffer
#[derive(Debug, Clone)]
pub struct JitterBufferConfig {
    /// cuánto tiempo queremos "acumular" antes de mostrar
    pub target_delay: Duration,

    /// cuánto tiempo máximo toleramos
    pub max_delay: Duration,
}

/// Implementación del jitter buffer
///
/// Guarda frames en un heap ordenado por timestamp.
/// El hilo reproductor retira frames cuando están listas.
pub struct JitterBuffer {
    heap: BinaryHeap<Reverse<JitterFrameWrapper>>,
    cfg: JitterBufferConfig,
    start_playout: Option<Instant>,
}

/// Wrapper necesario para ordenar por timestamp (menor -> primero)
#[derive(Debug, Clone)]
struct JitterFrameWrapper {
    timestamp: u32,
    arrival: Instant,
    data: Vec<u8>,
}

impl PartialEq for JitterFrameWrapper {
    fn eq(&self, other: &Self) -> bool {
        self.timestamp == other.timestamp
    }
}
impl Eq for JitterFrameWrapper {}

impl PartialOrd for JitterFrameWrapper {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for JitterFrameWrapper {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        other.timestamp.cmp(&self.timestamp)
    }
}

impl JitterBuffer {
    pub fn new(cfg: JitterBufferConfig) -> Self {
        Self {
            heap: BinaryHeap::new(),
            cfg,
            start_playout: None,
        }
    }

    /// Inserto un frame nuevo
    pub fn push(&mut self, frame: JitterFrame) {
        self.heap.push(Reverse(JitterFrameWrapper {
            timestamp: frame.timestamp_90khz,
            arrival: frame.arrival,
            data: frame.data,
        }));
    }

    /// Intento obtener un frame listo para reproducir.
    ///
    /// now es el instante actual (Instant::now()).
    ///
    /// Devuelve Some(frame) si ya pasó el target delay.
    pub fn pop_ready(&mut self, now: Instant) -> Option<Vec<u8>> {
        if self.heap.is_empty() {
            return None;
        }

        // Inicializamos el tiempo de reproducción
        if self.start_playout.is_none() {
            self.start_playout = Some(now);
        }

        let start = self.start_playout.unwrap();

        // Si aún no alcanzamos el delay inicial -> no liberar frames
        if now.duration_since(start) < self.cfg.target_delay {
            return None;
        }

        // El siguiente frame más antiguo está listo
        let Reverse(wrapper) = self.heap.peek().unwrap();

        // Si el frame está demasiado retrasado, saltarlo
        if now.duration_since(wrapper.arrival) > self.cfg.max_delay {
            // descarto
            let Reverse(old) = self.heap.pop().unwrap();
            eprintln!(
                "[JITTER] descartado por exceso de delay. ts={} delay_ms={}",
                old.timestamp,
                now.duration_since(old.arrival).as_millis()
            );
            return None;
        }

        // listo para reproducir
        let Reverse(w) = self.heap.pop().unwrap();
        Some(w.data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Helper para crear frames dummys
    fn make_frame(ts: u32, arrival: Instant, data_byte: u8) -> JitterFrame {
        JitterFrame {
            timestamp_90khz: ts,
            arrival,
            data: vec![data_byte], // Uso un byte para identificar el frame
        }
    }

    #[test]
    fn test_ordering_min_heap() {
        // Verifico que salen ordenados por timestamp (menor a mayor)
        // aunque entren desordenados.

        let cfg = JitterBufferConfig {
            target_delay: Duration::from_millis(0), // Sin delay para este test
            max_delay: Duration::from_secs(5),
        };
        let mut jb = JitterBuffer::new(cfg);
        let now = Instant::now();

        // Insertao desordenado: 300, 100, 200
        jb.push(make_frame(300, now, 3));
        jb.push(make_frame(100, now, 1));
        jb.push(make_frame(200, now, 2));

        // Salen: 300 -> 200 -> 100
        let f1 = jb.pop_ready(now).expect("Debería salir frame 300");
        assert_eq!(f1[0], 3);

        let f2 = jb.pop_ready(now).expect("Debería salir frame 200");
        assert_eq!(f2[0], 2);

        let f3 = jb.pop_ready(now).expect("Debería salir frame 100");
        assert_eq!(f3[0], 1);

        assert!(jb.pop_ready(now).is_none());
    }

    #[test]
    fn test_target_delay_buffering() {
        // El buffer debe retener los frames hasta cumplir el target_delay
        let cfg = JitterBufferConfig {
            target_delay: Duration::from_millis(100), // 100ms de buffer
            max_delay: Duration::from_secs(1),
        };
        let mut jb = JitterBuffer::new(cfg);

        // Simulamos el tiempo con Instant
        let start = Instant::now();

        // Llega un frame en t=0
        jb.push(make_frame(1000, start, 1));

        // Intento sacar inmediatamente (t=0)
        // Debería ser None porque target_delay es 100ms
        assert!(
            jb.pop_ready(start).is_none(),
            "No debería salir inmediatamente"
        );

        // Intento sacar a los 50ms
        let t_50 = start + Duration::from_millis(50);
        assert!(
            jb.pop_ready(t_50).is_none(),
            "A los 50ms debería seguir buffereando"
        );

        // Intento sacar a los 101ms (Pasó el target)
        let t_101 = start + Duration::from_millis(101);
        let res = jb.pop_ready(t_101);
        assert!(res.is_some(), "A los 101ms debería liberar el frame");
        assert_eq!(res.unwrap()[0], 1);
    }

    #[test]
    fn test_max_delay_discard() {
        // Si un frame es demasiado viejo respecto al 'now', se descarta.
        let cfg = JitterBufferConfig {
            target_delay: Duration::from_millis(0),
            max_delay: Duration::from_millis(200), // Max 200ms de vida
        };
        let mut jb = JitterBuffer::new(cfg);
        let now = Instant::now();

        // Frame viejo: Llegó hace 300ms
        let arrival_old = now - Duration::from_millis(300);
        jb.push(make_frame(100, arrival_old, 1));

        let arrival_new = now - Duration::from_millis(10);
        jb.push(make_frame(200, arrival_new, 2));

        // 1er pop: sale el más nuevo (ts=200)
        let res1 = jb
            .pop_ready(now)
            .expect("Debería salir el frame nuevo (ts=200)");
        assert_eq!(res1[0], 2);

        // 2do pop: queda el viejo (ts=100), se descarta por max_delay -> None
        assert!(
            jb.pop_ready(now).is_none(),
            "El frame viejo debería descartarse por exceder max_delay"
        );

        // 3er pop: buffer vacío
        assert!(jb.pop_ready(now).is_none());
    }

    #[test]
    fn test_empty_buffer() {
        let cfg = JitterBufferConfig {
            target_delay: Duration::from_millis(50),
            max_delay: Duration::from_millis(200),
        };
        let mut jb = JitterBuffer::new(cfg);
        let now = Instant::now();

        assert!(jb.pop_ready(now).is_none());
    }

    #[test]
    fn test_complex_sequence() {
        let cfg = JitterBufferConfig {
            target_delay: Duration::from_millis(50),
            max_delay: Duration::from_millis(500),
        };
        let mut jb = JitterBuffer::new(cfg);
        let start = Instant::now();

        // Llegan paquetes (simulo llegada en t=0)
        jb.push(make_frame(10, start, 10));
        jb.push(make_frame(30, start, 30));
        jb.push(make_frame(20, start, 20)); // Desordenado

        // t=0: Buffering...
        assert!(jb.pop_ready(start).is_none());

        // t=60ms: Ya pasó el target delay, debería poder drenar todo en orden
        let now = start + Duration::from_millis(60);

        let f1 = jb.pop_ready(now).unwrap();
        assert_eq!(f1[0], 30);

        let f2 = jb.pop_ready(now).unwrap();
        assert_eq!(f2[0], 20);

        let f3 = jb.pop_ready(now).unwrap();
        assert_eq!(f3[0], 10);

        assert!(jb.pop_ready(now).is_none());
    }
}
