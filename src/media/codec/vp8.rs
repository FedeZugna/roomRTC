use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app::{AppSink, AppSrc};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};

use super::super::constants::{VIDEO_HEIGHT, VIDEO_WIDTH};

#[derive(Clone, Debug)]
pub struct EncodedFrame {
    pub data: Vec<u8>,
}

// Alias para mejorar legibilidad de los tipos compartidos de RGBA sugerido por clippy
type SharedRgbaBuffer = Arc<Mutex<Option<Vec<u8>>>>;
type RgbaConfig = Option<(SharedRgbaBuffer, u32, u32)>;

fn gst_init_once() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        gst::init().expect("No se pudo inicializar GStreamer");
    });
}

/// Inicia el ENCODER VP8 y devuelve el Receiver para recibir los frames VP8
/// Si se provee `rgba_config`, se extraen frames RGBA además de VP8
pub fn start_vp8_encoder_with_rgba(
    fps: u32,
    source_pipeline: &str,
    rgba_config: RgbaConfig,
) -> Result<Receiver<EncodedFrame>, Box<dyn std::error::Error>> {
    gst_init_once();

    let (tx, rx) = mpsc::channel::<EncodedFrame>();

    let (width, height) = match &rgba_config {
        Some((_, w, h)) => (*w, *h),
        None => (VIDEO_WIDTH, VIDEO_HEIGHT),
    };

    let pipeline_desc = format!(
        "{src} \
     ! videoconvert \
     ! videoscale \
     ! video/x-raw,framerate={fps}/1,width={w},height={h} \
     ! tee name=t \
     t. ! queue \
        ! vp8enc deadline=1 error-resilient=default keyframe-max-dist={kf} \
        ! appsink name=outsink emit-signals=true sync=false max-buffers=10 drop=true \
     t. ! queue \
        ! videoconvert \
        ! video/x-raw,format=RGBA \
        ! appsink name=rgbasink emit-signals=true sync=false max-buffers=2 drop=true",
        src = source_pipeline,
        fps = fps,
        w = width,
        h = height,
        kf = (fps.saturating_mul(2)).max(60),
    );

    let pipeline = gst::parse_launch(&pipeline_desc)?
        .downcast::<gst::Pipeline>()
        .expect("No se pudo crear pipeline ENCODER VP8");

    let appsink = pipeline
        .by_name("outsink")
        .expect("No se encontró appsink 'outsink'")
        .downcast::<AppSink>()
        .expect("'outsink' no es AppSink");

    // Aseguro caps de salida VP8 en el appsink
    appsink.set_caps(Some(&gst::Caps::builder("video/x-vp8").build()));

    // Callback para cada sample del encoder que enviamos
    appsink.set_callbacks(
        gstreamer_app::AppSinkCallbacks::builder()
            .new_sample(move |sink| {
                let sample = sink.pull_sample().map_err(|_| gst::FlowError::Eos)?;

                let buffer = sample.buffer().ok_or(gst::FlowError::Error)?;
                let map = buffer.map_readable().map_err(|_| gst::FlowError::Error)?;
                let bytes = map.as_slice().to_vec();

                if tx.send(EncodedFrame { data: bytes }).is_err() {
                    return Err(gst::FlowError::Eos);
                }

                Ok(gst::FlowSuccess::Ok)
            })
            .build(),
    );

    // Si se pidió captura RGBA, configurar el segundo appsink
    if let Some((rgba_buffer, _, _)) = rgba_config {
        let rgbasink = pipeline
            .by_name("rgbasink")
            .expect("No se encontró appsink 'rgbasink'")
            .downcast::<AppSink>()
            .expect("'rgbasink' no es AppSink");

        rgbasink.set_callbacks(
            gstreamer_app::AppSinkCallbacks::builder()
                .new_sample(move |sink| {
                    if let Ok(sample) = sink.pull_sample()
                        && let Some(buffer) = sample.buffer()
                        && let Ok(map) = buffer.map_readable()
                    {
                        let rgba_data = map.as_slice().to_vec();
                        if let Ok(mut frame) = rgba_buffer.lock() {
                            *frame = Some(rgba_data);
                        }
                    }
                    Ok(gst::FlowSuccess::Ok)
                })
                .build(),
        );
    }

    pipeline.set_state(gst::State::Playing)?;
    println!("[VP8] Encoder iniciado @ {fps} FPS");
    Ok(rx)
}
/// Inicia el DECODER VP8 y devuelve el Sender para empujar los frames VP8
pub fn start_vp8_decoder_sink(
    fps: u32,
) -> Result<Sender<EncodedFrame>, Box<dyn std::error::Error>> {
    gst_init_once();

    let (tx, rx) = mpsc::channel::<EncodedFrame>();

    // appsrc (VP8 bytes) -> vp8dec -> videoconvert -> fakesink (sin ventana extra)
    let pipeline_desc = format!(
        "appsrc name=insrc is-live=true do-timestamp=true format=time \
         caps=video/x-vp8,framerate={}/1 \
         ! vp8dec ! videoconvert ! fakesink sync=false",
        fps.max(1)
    );

    let pipeline = gst::parse_launch(&pipeline_desc)?
        .downcast::<gst::Pipeline>()
        .expect("No se pudo crear pipeline DECODER VP8");

    let appsrc = pipeline
        .by_name("insrc")
        .expect("No se encontró appsrc 'insrc'")
        .downcast::<AppSrc>()
        .expect("'insrc' no es AppSrc");

    // Formato de tiempo para timestamps ascendentes
    appsrc.set_format(gst::Format::Time);

    // Hilo que empuja buffers VP8 al decoder
    let pipeline_for_thread = pipeline.clone();
    std::thread::spawn(move || {
        pipeline_for_thread
            .set_state(gst::State::Playing)
            .expect("No se pudo poner PLAYING al decoder");
        for frame in rx {
            let buffer = gst::Buffer::from_slice(frame.data);
            if let Err(e) = appsrc.push_buffer(buffer) {
                eprintln!("[VP8] appsrc push error: {e:?}");
                break;
            }
        }
        let _ = pipeline_for_thread.set_state(gst::State::Null);
    });

    println!("[VP8] Decoder iniciado @ {fps} FPS");
    Ok(tx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    // -----------------------------
    // TEST 1: El encoder se construye
    // -----------------------------
    #[test]
    fn test_encoder_pipeline_builds() {
        gst_init_once();

        let rgba_buf: SharedRgbaBuffer = Arc::new(Mutex::new(None));
        let rgba_cfg = Some((rgba_buf, VIDEO_WIDTH, VIDEO_HEIGHT));

        let result = start_vp8_encoder_with_rgba(
            30,
            "fakesrc num-buffers=1 ! video/x-raw,format=RGB,width=640,height=480",
            rgba_cfg,
        );

        assert!(result.is_ok());
        let rx = result.unwrap();

        // El receiver existe y sirve para recibir frames VP8
        assert!(
            rx.recv_timeout(std::time::Duration::from_millis(10))
                .is_err()
        );
    }

    // -----------------------------------------------------
    // TEST 2: El decoder se construye y retorna un Sender
    // -----------------------------------------------------
    #[test]
    fn test_decoder_pipeline_builds() {
        gst_init_once();

        let result = start_vp8_decoder_sink(30);
        assert!(result.is_ok());

        let tx = result.unwrap();

        // Debemos poder enviar sin panic (consume test thread)
        let send_result = tx.send(EncodedFrame {
            data: vec![1, 2, 3, 4],
        });
        assert!(send_result.is_ok());
    }

    // ---------------------------------------------------------------------
    // TEST 3: El RGBA buffer opcional funciona sin bloquear ni panickear
    // ---------------------------------------------------------------------
    #[test]
    fn test_rgba_buffer_mutex() {
        let rgba: SharedRgbaBuffer = Arc::new(Mutex::new(None));

        {
            let mut lock = rgba.lock().unwrap();
            *lock = Some(vec![100, 200, 50, 25]);
        }

        // Podemos leerlo nuevamente
        let read = rgba.lock().unwrap();
        assert!(read.is_some());
        assert_eq!(read.as_ref().unwrap().len(), 4);
    }

    // ---------------------------------------------------------
    // TEST 4: start_vp8_encoder sin RGBA NO falla
    // ---------------------------------------------------------
    #[test]
    fn test_encoder_no_rgba() {
        gst_init_once();

        let result = start_vp8_encoder_with_rgba(
            30,
            "fakesrc num-buffers=1 ! video/x-raw,format=RGB,width=640,height=480",
            None,
        );

        assert!(result.is_ok());
    }

    // ---------------------------------------------------------
    // TEST 5: El pipeline del decoder tiene un appsrc válido
    // ---------------------------------------------------------
    #[test]
    fn test_decoder_has_appsrc() {
        gst_init_once();

        let pipeline_desc = format!(
            "appsrc name=insrc is-live=true do-timestamp=true format=time \
             caps=video/x-vp8,framerate=30/1 \
             ! vp8dec ! videoconvert ! fakesink sync=false"
        );

        let pipeline = gst::parse_launch(&pipeline_desc)
            .unwrap()
            .downcast::<gst::Pipeline>()
            .unwrap();

        let appsrc = pipeline.by_name("insrc");
        assert!(appsrc.is_some());
    }
}
