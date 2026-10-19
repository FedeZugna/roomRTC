//! Pipeline de video para codificación y decodificación.
//!
//! Este módulo contiene las funciones para configurar los pipelines de GStreamer
//! para codificar video desde una camara y decodificar video
//! recibido para su visualización.

use crate::client::types::{ClientChannels, ClientResult};
use crate::media::codec::vp8::{self, EncodedFrame};
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app::AppSink;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

/// Inicia un decodificador VP8 que convierte frames codificados a RGBA para mostrar en ventana de iced.
///
/// Crea un pipeline GStreamer que recibe frames VP8, los decodifica y los convierte
/// a formato RGBA, almacenando el resultado en un buffer compartido.
///
/// # Argumentos
/// * `fps` - Frames por segundo esperados.
/// * `width` - Ancho del video en píxeles.
/// * `height` - Alto del video en píxeles.
/// * `frame_buffer` - Buffer compartido donde se almacenarán los frames RGBA decodificados.
///
/// # Retorna
/// * `Ok(Sender<EncodedFrame>)` - Canal para enviar frames codificados al decodificador.
/// * `Err` - Error al configurar el pipeline de GStreamer.
pub fn start_rgba_decoder_for_display(
    fps: u32,
    width: u32,
    height: u32,
    frame_buffer: Arc<Mutex<Option<Vec<u8>>>>,
) -> Result<Sender<EncodedFrame>, Box<dyn std::error::Error>> {
    let (tx, rx) = std::sync::mpsc::channel::<EncodedFrame>();
    let pipeline_desc = format!(
        "appsrc name=insrc is-live=true do-timestamp=true format=time caps=video/x-vp8,framerate={}/1 ! vp8dec ! videoconvert ! video/x-raw,format=RGBA,width={},height={} ! appsink name=sink emit-signals=true sync=false max-buffers=2 drop=true",
        fps, width, height
    );
    let pipeline = gst::parse_launch(&pipeline_desc)?
        .downcast::<gst::Pipeline>()
        .map_err(|_| "Error pipeline")?;
    let appsrc = pipeline
        .by_name("insrc")
        .ok_or("No appsrc")?
        .downcast::<gstreamer_app::AppSrc>()
        .map_err(|_| "No AppSrc")?;
    appsrc.set_format(gst::Format::Time);
    let appsink = pipeline
        .by_name("sink")
        .ok_or("No appsink")?
        .downcast::<AppSink>()
        .map_err(|_| "No AppSink")?;

    appsink.set_callbacks(
        gstreamer_app::AppSinkCallbacks::builder()
            .new_sample(move |sink| {
                let sample = sink.pull_sample().map_err(|_| gst::FlowError::Eos)?;
                let buffer = sample.buffer().ok_or(gst::FlowError::Error)?;
                let map = buffer.map_readable().map_err(|_| gst::FlowError::Error)?;
                if let Ok(mut frame) = frame_buffer.lock() {
                    *frame = Some(map.as_slice().to_vec());
                }
                Ok(gst::FlowSuccess::Ok)
            })
            .build(),
    );

    pipeline.set_state(gst::State::Playing)?;
    std::thread::spawn(move || {
        for frame in rx {
            if appsrc
                .push_buffer(gst::Buffer::from_slice(frame.data))
                .is_err()
            {
                break;
            }
        }
        let _ = pipeline.set_state(gst::State::Null);
    });
    Ok(tx)
}

/// Configura el cliente de video con encoder y decoder.
///
/// Inicializa los pipelines de codificación y decodificación de video,
/// creando los canales necesarios para el flujo de datos y los buffers
/// compartidos para los frames locales y remotos.
///
/// # Argumentos
/// * `fps` - Frames por segundo para codificación/decodificación.
/// * `source_pipeline` - String con la descripción del pipeline de GStreamer para la fuente de video.
/// * `width` - Ancho del video en píxeles.
/// * `height` - Alto del video en píxeles.
///
/// # Retorna
/// * `Ok(ClientChannels)` - Tupla con los canales y buffers necesarios:
///   - Receiver de frames codificados (desde el encoder)
///   - Sender de frames codificados (hacia el decoder)
///   - Buffer de frames RGBA locales (cámara local)
///   - Buffer de frames RGBA remotos (peer remoto)
/// * `Err` - Error al configurar los pipelines.
pub fn setup_iced_video_client(
    fps: u32,
    source_pipeline: &str,
    width: u32,
    height: u32,
) -> ClientResult<ClientChannels> {
    let local = Arc::new(Mutex::new(None));
    let remote = Arc::new(Mutex::new(None));
    let rx_enc = vp8::start_vp8_encoder_with_rgba(
        fps,
        source_pipeline,
        Some((Arc::clone(&local), width, height)),
    )?;
    let tx_dec = start_rgba_decoder_for_display(fps, width, height, Arc::clone(&remote))?;
    Ok((rx_enc, tx_dec, local, remote))
}
