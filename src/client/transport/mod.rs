//! Módulo de transporte para DTLS y SCTP.
pub mod dtls_manager;
pub mod sctp_chat;
#[cfg(test)]
pub(crate) mod sctp_harness;

pub use dtls_manager::spawn_dtls_handshake_thread;
