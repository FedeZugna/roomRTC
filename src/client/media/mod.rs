//! Módulo de medios para el cliente.
pub mod config;
pub mod ice_client;
pub mod video_pipeline;

pub use config::MediaClientConfig;
pub use ice_client::ClienteICELite;
pub use video_pipeline::{setup_iced_video_client, start_rgba_decoder_for_display};
