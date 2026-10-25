# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

RoomRTC is a peer-to-peer video-call app written in Rust (FIUBA "Taller de Programación" group project). It has a TLS signaling server, an `iced` GUI client, and a WebRTC-like stack built by hand: ICE-lite, DTLS, RTP/RTCP, and SCTP for file transfer. Code comments, logs and docs are in Spanish.

## Commands

```bash
cargo build
cargo test                                   # all tests (unit + tests/*.rs)
cargo test <name_substring>                  # single test, e.g. cargo test chat_1_3
cargo test --test t_sdp                      # one integration test file
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings    # baseline is warning-free
```

Running (from the repo root, because paths such as `certs/...` and `users.txt` are relative to the working directory):

```bash
cargo run --bin server                        # TLS signaling on 127.0.0.1:8443
cargo run --bin cliente src/config/cliente1.config        # Linux
cargo run --bin cliente src/config/cliente1_mac.config    # macOS (avfvideosrc)
# second peer: cliente2*.config
```

The README's `cliente controlled|controlling ...` usage is out of date. The client now takes one argument, the `.config` path. Its role is decided by signaling.

System dependencies: GStreamer, OpenCV/clang (`libopencv-dev clang libclang-dev pkg-config` on Linux) and OpenSSL. `certs/` (gitignored) must contain `localhost+2.pem`, `localhost+2-key.pem` (server) and `rootCA.pem` (client), for example generated with `mkcert localhost 127.0.0.1 ::1`.

`tests/t_signaling.rs` spawns the real `server` binary (`CARGO_BIN_EXE_server`) on fixed port 8443 and generates certs with OpenSSL. It fails if a server is already running on that port.

Baseline outputs (test/clippy/fmt) are recorded in `docs/baseline/`.

## Architecture

**Two binaries carry most of the logic.** `src/bin/server.rs` and `src/bin/cliente.rs` (~1.2k and ~1.8k lines) hold much of the runtime behavior. The library (`src/lib.rs`) holds reusable protocol pieces. `src/main.rs` is only a demo.

- **Signaling server** (`src/bin/server.rs`): rustls TLS server. It speaks the length-prefixed JSON protocol (`[u32 BE length][JSON]`) described in the README, using the `Message` enum (Register/Login/GetUserList/CallRequest/CallEnd/...). Users persist in `users.txt`. During a call it relays SDP/ICE/fingerprint data between peers (`relay_signaling`). The client keeps its own copy of the protocol in `src/client/signaling/messages.rs`, and both copies must stay in sync.
- **Client `main`**: generates a self-signed DTLS identity and fingerprint. It spawns an auth/signaling thread (`TlsClient` → server) and parses the `.config` file. Then it calls `run_media_client`, which runs the `iced` UI on the main thread.
  - The `.config` files are **SDP-formatted** (`src/config/*.config`, parsed by `src/sdp`). The media `source` attribute is a GStreamer pipeline. The remote port is local ±1, picked by `session_id` parity.
  - The signaling thread sends `(is_controlling, remote_fingerprint)` over `role_rx` once a call is accepted. That starts the DTLS handshake (`client/transport/dtls_manager.rs`).
- **Threads and channels**: `run_media_client` starts tx/rx threads over a shared `DtlsStream`. The rx thread demultiplexes RTP from SCTP. There are also RTCP, jitter-buffer and audio threads. The UI talks to the network only through mpsc channels:
  - `SignalingCommand` / `SignalingResponse` for lobby/call control.
  - `SctpCommand` (UI → net) / `SctpNotification` (net → UI) for data-channel features.
  - These enums, `AppMessage` and `AppScreen` live in `src/media/unified_app.rs`.
- **SCTP streams** (`sctp-proto` over DTLS): stream 0 carries text control messages for file transfer (`OFFER`, accept, reject), and stream 1 carries file content (`[len u16][name][bytes]`). New data features must use a different stream so they don't interfere with these.
- **Media**: VP8 codec (`media/codec`) → "RTP-lite" packetization and reassembly (`src/rtp`) → `jitter_buffer` → shared RGBA frame shown by `iced`. RTCP stats are in `src/rtcp`. ICE candidates and STUN are in `candidates.rs` and `stun_helper`.
- **UI**: `media/unified_app.rs` is the one `iced` app. It moves through the Login → Register → Lobby → VideoCall screens. The per-screen viewers sit beside it in `media/`.
- **Logging**: a local path crate `src/log` (re-exported as `room_rtc::log`) writes to `logger.log`.

## OpenSpec workflow

Feature work follows OpenSpec (`openspec/config.yaml`, `/opsx:*` commands and `openspec-*` skills). The current branch `feat/in-call-chat` adds in-call text chat. Its scope and acceptance tests are defined in `docs/chat/roomRTC_todo.md` and `docs/chat/test_aceptacion.md`. Rules from the config that apply when implementing:

- Write all OpenSpec artifacts in Spanish.
- Do not modify `src/bin/server.rs` or the signaling `Message` enum. The server takes no part in chat.
- Chat must not affect file transfer (SCTP streams 0 and 1).
- Acceptance tests are named `chat_<hito>_<n>_<descripcion>`, numbered as in `test_aceptacion.md`.
- Within each milestone, write the tests first, then the implementation. Finish one milestone, gates included, before starting the next.
- New chat code uses no `unwrap`/`expect`/`panic!`. Errors are returned as `Result`.
- Never edit or delete tests to hide failures. Update the spec first.
- The gates are `cargo test`, `cargo fmt --all -- --check` and `cargo clippy --all-targets -- -D warnings`. Show their real output.
- Milestone 0 (exploration) conclusions go in `docs/chat/exploracion.md`. Exploration is read-only: do not modify production code.
- The limits of 4096 bytes per message and 200 history entries are provisional until confirmed in milestone 0.

## Known limitations (macOS)

The project was written on Ubuntu and is now developed on macOS. Two known problems come from that difference and are **out of scope for this branch**: the camera opens but is not shown in the call, and the other client hangs when one client closes. Do not try to fix them. If they matter for a finding (for example, call lifecycle), only describe them.
