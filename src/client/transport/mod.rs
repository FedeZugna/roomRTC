//! Módulo de transporte para DTLS y SCTP.
pub mod dtls_manager;

pub use dtls_manager::spawn_dtls_handshake_thread;
