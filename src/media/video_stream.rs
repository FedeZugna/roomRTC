use gstreamer as gst;
use gstreamer::prelude::*;
use std::net::UdpSocket;
//use std::thread;
//use std::time::Duration;

/// Inicializa GStreamer una sola vez
fn init_gstreamer() {
    // Si ya está inicializado, no falla
    gst::init().expect("No se pudo inicializar GStreamer");
}

/// Captura la cámara local y envía el video comprimido por UDP (RTP/VP8)
pub fn capturar_y_enviar(
    _socket: &UdpSocket,
    destino: std::net::SocketAddr,
    usar_camara_real: bool,
    rol: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    init_gstreamer();

    println!(
        "[{}] Iniciando pipeline de envío hacia {}:{} (usar_camara_real={})",
        rol,
        destino.ip(),
        destino.port(),
        usar_camara_real
    );

    // webcam real
    let src_elemento = if usar_camara_real {
        "v4l2src device=/dev/video0"
    } else {
        // cámara virtual simple negrta
        //"videotestsrc pattern=black"
        //"videoteststc pattern=smpte"
        "v4l2src device=/dev/video2"
    };

    // Pipeline: fuente -> conversión -> VP8 -> RTP -> udpsink
    let pipeline_desc = format!(
        "{src} ! videoconvert ! vp8enc deadline=1 ! rtpvp8pay pt=96 ! udpsink host={host} port={port} sync=false async=false",
        src = src_elemento,
        host = destino.ip(),
        port = destino.port(),
    );

    println!("[{}] Pipeline envío: {}", rol, pipeline_desc);

    let pipeline = gst::parse_launch(&pipeline_desc)?
        .downcast::<gst::Pipeline>()
        .expect("No se pudo crear el pipeline de envío");

    pipeline.set_state(gst::State::Playing)?;

    // Mantiene vivo hasta Ctrl+C o hasta que el peer cierre
    let bus = pipeline.bus().expect("Sin bus en pipeline de envío");
    for msg in bus.iter_timed(gst::ClockTime::NONE) {
        use gst::MessageView;
        match msg.view() {
            MessageView::Eos(..) => {
                println!("[{}] Fin del envío.", rol);
                break;
            }
            MessageView::Error(err) => {
                eprintln!(
                    "[{}] Error de envío: {} ({:?})",
                    rol,
                    err.error(),
                    err.debug()
                );
                break;
            }
            _ => {}
        }
    }

    let _ = pipeline.set_state(gst::State::Null);
    Ok(())
}

/// Recibe video por UDP (RTP/VP8) y lo muestra en una ventana
pub fn recibir_y_mostrar(puerto_local: u16, rol: &str) -> Result<(), Box<dyn std::error::Error>> {
    init_gstreamer();

    println!("\n \n  [{}] Ventana de Video Activa\n \n", rol);

    println!(
        "[{}] Iniciando pipeline de recepción en UDP/{}",
        rol, puerto_local
    );

    // Pipeline: udpsrc (RTP VP8) -> depay -> decodificador -> convert -> sink
    let pipeline_desc = format!(
        "udpsrc port={port} caps=\"application/x-rtp, media=video, encoding-name=VP8, payload=96\" \
         ! rtpvp8depay ! vp8dec ! videoconvert ! autovideosink sync=false",
        port = puerto_local
    );

    println!("[{}] Pipeline recepción: {}", rol, pipeline_desc);

    let pipeline = gst::parse_launch(&pipeline_desc)?
        .downcast::<gst::Pipeline>()
        .expect("No se pudo crear el pipeline de recepción");

    pipeline.set_state(gst::State::Playing)?;

    let bus = pipeline
        .bus()
        .expect("No se pudo obtener el bus del pipeline");

    println!("[{}] Mostrando video recibido...", rol);

    for msg in bus.iter_timed(gst::ClockTime::NONE) {
        use gst::MessageView;

        match msg.view() {
            MessageView::Eos(..) => {
                println!("[{}] Fin de la transmisión recibida.", rol);
                break;
            }
            MessageView::Error(err) => {
                eprintln!(
                    "[{}] Error de recepción: {} ({:?})",
                    rol,
                    err.error(),
                    err.debug()
                );
                break;
            }
            _ => {}
        }
    }

    let _ = pipeline.set_state(gst::State::Null);
    Ok(())
}
