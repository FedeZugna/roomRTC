use room_rtc::client::{Message, TlsClient, UserStatus};

use openssl::asn1::{Asn1Integer, Asn1Time};
use openssl::bn::BigNum;
use openssl::hash::MessageDigest;
use openssl::pkey::PKey;
use openssl::rsa::Rsa;
use openssl::x509::extension::{BasicConstraints, ExtendedKeyUsage, KeyUsage, SubjectAlternativeName};
use openssl::x509::{X509NameBuilder, X509};

use std::fs;
use std::io;
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

const ADDR: &str = "127.0.0.1:8443";

fn global_test_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(())).lock().unwrap()
}

struct ServerGuard {
    child: Child,
}
impl Drop for ServerGuard {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn create_unique_temp_dir(prefix: &str) -> io::Result<PathBuf> {
    let base = std::env::temp_dir();
    for i in 0..200u32 {
        let candidate = base.join(format!(
            "{}_{}_{}_{}",
            prefix,
            std::process::id(),
            i,
            (std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos())
        ));
        match fs::create_dir_all(&candidate) {
            Ok(_) => return Ok(candidate),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "No pude crear temp dir único",
    ))
}

/// Genero CA + cert de servidor para "localhost"
/// 
/// - certs/localhost+2.pem     
/// - certs/localhost+2-key.pem
/// - certs/rootCA.pem
fn write_test_certs(workdir: &Path) -> io::Result<()> {
    let certs_dir = workdir.join("certs");
    fs::create_dir_all(&certs_dir)?;

    let ca_key = PKey::from_rsa(Rsa::generate(2048).unwrap()).unwrap();

    let mut ca_name = X509NameBuilder::new().unwrap();
    ca_name.append_entry_by_text("CN", "RoomRTC Test CA").unwrap();
    let ca_name = ca_name.build();

    let mut ca_builder = X509::builder().unwrap();
    ca_builder.set_version(2).unwrap();

    let mut ca_serial_bn = BigNum::new().unwrap();
    ca_serial_bn
        .rand(64, openssl::bn::MsbOption::MAYBE_ZERO, false)
        .unwrap();
    let ca_serial = Asn1Integer::from_bn(&ca_serial_bn).unwrap();
    ca_builder.set_serial_number(&ca_serial).unwrap();

    ca_builder.set_subject_name(&ca_name).unwrap();
    ca_builder.set_issuer_name(&ca_name).unwrap();
    ca_builder.set_pubkey(&ca_key).unwrap();
    ca_builder
        .set_not_before(&Asn1Time::days_from_now(0).unwrap())
        .unwrap();
    ca_builder
        .set_not_after(&Asn1Time::days_from_now(365).unwrap())
        .unwrap();

    let ca_basic = BasicConstraints::new().critical().ca().build().unwrap();
    ca_builder.append_extension(ca_basic).unwrap();

    let ca_key_usage = KeyUsage::new().key_cert_sign().crl_sign().build().unwrap();
    ca_builder.append_extension(ca_key_usage).unwrap();

    ca_builder.sign(&ca_key, MessageDigest::sha256()).unwrap();
    let ca_cert = ca_builder.build();

    let server_key = PKey::from_rsa(Rsa::generate(2048).unwrap()).unwrap();

    let mut srv_name = X509NameBuilder::new().unwrap();
    srv_name.append_entry_by_text("CN", "localhost").unwrap();
    let srv_name = srv_name.build();

    let mut srv_builder = X509::builder().unwrap();
    srv_builder.set_version(2).unwrap();

    let mut srv_serial_bn = BigNum::new().unwrap();
    srv_serial_bn
        .rand(64, openssl::bn::MsbOption::MAYBE_ZERO, false)
        .unwrap();
    let srv_serial = Asn1Integer::from_bn(&srv_serial_bn).unwrap();
    srv_builder.set_serial_number(&srv_serial).unwrap();

    srv_builder.set_subject_name(&srv_name).unwrap();
    srv_builder.set_issuer_name(ca_cert.subject_name()).unwrap();
    srv_builder.set_pubkey(&server_key).unwrap();
    srv_builder
        .set_not_before(&Asn1Time::days_from_now(0).unwrap())
        .unwrap();
    srv_builder
        .set_not_after(&Asn1Time::days_from_now(365).unwrap())
        .unwrap();

    let srv_basic = BasicConstraints::new().critical().build().unwrap(); 
    srv_builder.append_extension(srv_basic).unwrap();

    let srv_ku = KeyUsage::new()
        .digital_signature()
        .key_encipherment()
        .build()
        .unwrap();
    srv_builder.append_extension(srv_ku).unwrap();

    let srv_eku = ExtendedKeyUsage::new().server_auth().build().unwrap();
    srv_builder.append_extension(srv_eku).unwrap();

    let san_ext = {
        let ctx = srv_builder.x509v3_context(Some(&ca_cert), None);
        let mut san = SubjectAlternativeName::new();
        san.dns("localhost");
        san.ip("127.0.0.1");
        san.build(&ctx).unwrap()
    };
    srv_builder.append_extension(san_ext).unwrap();

    srv_builder.sign(&ca_key, MessageDigest::sha256()).unwrap();
    let srv_cert = srv_builder.build();

    let ca_pem = ca_cert.to_pem().unwrap();
    let srv_pem = srv_cert.to_pem().unwrap();
    let chain_pem = [srv_pem.as_slice(), ca_pem.as_slice()].concat();

    fs::write(certs_dir.join("rootCA.pem"), ca_pem)?;
    fs::write(certs_dir.join("localhost+2.pem"), chain_pem)?;
    fs::write(
        certs_dir.join("localhost+2-key.pem"),
        server_key.private_key_to_pem_pkcs8().unwrap(),
    )?;

    Ok(())
}

fn assert_port_free() {
    if TcpStream::connect(ADDR).is_ok() {
        panic!(
            "El puerto {} ya está ocupado. Cerrá cualquier server previo y reintentá.",
            ADDR
        );
    }
}

fn wait_for_listen(timeout: Duration) {
    let start = Instant::now();
    loop {
        if TcpStream::connect(ADDR).is_ok() {
            return;
        }
        if start.elapsed() > timeout {
            panic!("Timeout esperando que el server escuche en {}", ADDR);
        }
        thread::sleep(Duration::from_millis(30));
    }
}

fn wait_for_port_close(timeout: Duration) {
    let start = Instant::now();
    loop {
        if TcpStream::connect(ADDR).is_err() {
            return;
        }
        if start.elapsed() > timeout {
            panic!("Timeout esperando que el puerto {} se libere", ADDR);
        }
        thread::sleep(Duration::from_millis(30));
    }
}

fn spawn_server(workdir: &Path) -> ServerGuard {
    let exe = env!("CARGO_BIN_EXE_server");
    let child = Command::new(exe)
        .current_dir(workdir)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("No pude spawnear el binario server");

    ServerGuard { child }
}

fn connect_tls(workdir: &Path) -> TlsClient {
    let ca_path = workdir.join("certs").join("rootCA.pem");
    TlsClient::connect(ADDR, ca_path.to_str().unwrap(), "localhost")
        .expect("No pude conectar con TlsClient al server")
}

fn expect_success(m: Message) {
    match m {
        Message::Success { .. } => {}
        other => panic!("Esperaba Success y llegó: {:?}", other),
    }
}

fn expect_error(m: Message) {
    match m {
        Message::Error { .. } => {}
        other => panic!("Esperaba Error y llegó: {:?}", other),
    }
}

fn recv_eventually(client: &mut TlsClient, timeout: Duration) -> Message {
    let start = Instant::now();
    loop {
        match client.recv() {
            Ok(m) => return m,
            Err(e)
                if e.kind() == io::ErrorKind::TimedOut
                    || e.kind() == io::ErrorKind::WouldBlock =>
            {
                if start.elapsed() > timeout {
                    panic!("Timeout esperando mensaje del server");
                }
                thread::sleep(Duration::from_millis(15));
            }
            Err(e) => panic!("recv() error inesperado: {}", e),
        }
    }
}

fn get_user_list(client: &mut TlsClient) -> Vec<(String, UserStatus)> {
    client.send(&Message::GetUserList).unwrap();
    match client.recv().unwrap() {
        Message::UserListResponse { mut users } => {
            users.sort_by(|a, b| a.0.cmp(&b.0));
            users
        }
        other => panic!("Esperaba UserListResponse y llegó: {:?}", other),
    }
}

#[test]
fn signaling_end_to_end_register_login_call_and_persistence() {
    let _guard = global_test_lock(); 

    assert_port_free();

    let workdir = create_unique_temp_dir("room_rtc").unwrap();
    fs::write(workdir.join("users.txt"), b"").unwrap();
    write_test_certs(&workdir).unwrap();

    // Levanto server
    let mut server = spawn_server(&workdir);
    wait_for_listen(Duration::from_secs(4));

    // Conecto 2 clientes
    let mut test = connect_tls(&workdir);
    let mut test2 = connect_tls(&workdir);

    test.set_read_timeout(Some(Duration::from_millis(200))).unwrap();
    test2.set_read_timeout(Some(Duration::from_millis(200))).unwrap();

    // Register
    test.send(&Message::Register {
        username: "test".into(),
        password: "pw1".into(),
        email: Some("test@test.com".into()),
    })
    .unwrap();
    expect_success(test.recv().unwrap());

    test2.send(&Message::Register {
        username: "test2".into(),
        password: "pw2".into(),
        email: None,
    })
    .unwrap();
    expect_success(test2.recv().unwrap());

    // Login OK / FAIL
    test.send(&Message::Login {
        username: "test".into(),
        password: "pw1".into(),
    })
    .unwrap();
    expect_success(test.recv().unwrap());

    let mut bad = connect_tls(&workdir);
    bad.set_read_timeout(Some(Duration::from_millis(200))).unwrap();
    bad.send(&Message::Login {
        username: "test".into(),
        password: "WRONG".into(),
    })
    .unwrap();
    expect_error(bad.recv().unwrap());

    test2.send(&Message::Login {
        username: "test2".into(),
        password: "pw2".into(),
    })
    .unwrap();
    expect_success(test2.recv().unwrap());

    // UserList: ambos Available
    let users = get_user_list(&mut test);
    let mut map = std::collections::HashMap::new();
    for (u, st) in users {
        map.insert(u, st);
    }
    assert_eq!(map.get("test"), Some(&UserStatus::Available));
    assert_eq!(map.get("test2"), Some(&UserStatus::Available));

    // Llamada
    test.send(&Message::CallRequest {
        callee: "test2".into(),
        fingerprint: "FP_test".into(),
    })
    .unwrap();
    expect_success(test.recv().unwrap());

    // test2 recibe CallIncoming
    let incoming = recv_eventually(&mut test2, Duration::from_secs(2));
    match incoming {
        Message::CallIncoming { caller, fingerprint } => {
            assert_eq!(caller, "test");
            assert_eq!(fingerprint, "FP_test");
        }
        other => panic!("Esperaba CallIncoming y llegó: {:?}", other),
    }

    // test2 acepta
    test2.send(&Message::CallAccept {
        caller: "test".into(),
        fingerprint: "FP_test2".into(),
    })
    .unwrap();
    expect_success(test2.recv().unwrap());

    // test recibe CallAccept
    let accepted = recv_eventually(&mut test, Duration::from_secs(2));
    match accepted {
        Message::CallAccept { caller, fingerprint } => {
            assert_eq!(caller, "test2");
            assert_eq!(fingerprint, "FP_test2");
        }
        other => panic!("Esperaba CallAccept y llegó: {:?}", other),
    }

    // UserList: ambos Busy
    let users = get_user_list(&mut test);
    let mut map = std::collections::HashMap::new();
    for (u, st) in users {
        map.insert(u, st);
    }
    assert_eq!(map.get("test"), Some(&UserStatus::Busy));
    assert_eq!(map.get("test2"), Some(&UserStatus::Busy));

    // Terminar llamada
    test.send(&Message::CallEnd).unwrap();
    expect_success(test.recv().unwrap());

    // test2 recibe CallEnd async
    let ended = recv_eventually(&mut test2, Duration::from_secs(2));
    match ended {
        Message::CallEnd => {}
        other => panic!("Esperaba CallEnd y llegó: {:?}", other),
    }

    // UserList: vuelven a Available
    let users = get_user_list(&mut test);
    let mut map = std::collections::HashMap::new();
    for (u, st) in users {
        map.insert(u, st);
    }
    assert_eq!(map.get("test"), Some(&UserStatus::Available));
    assert_eq!(map.get("test2"), Some(&UserStatus::Available));

    // Rechazo de llamada
    test.send(&Message::CallRequest {
        callee: "test2".into(),
        fingerprint: "FP_test_2".into(),
    })
    .unwrap();
    expect_success(test.recv().unwrap());

    let incoming2 = recv_eventually(&mut test2, Duration::from_secs(2));
    match incoming2 {
        Message::CallIncoming { caller, .. } => assert_eq!(caller, "test"),
        other => panic!("Esperaba CallIncoming y llegó: {:?}", other),
    }

    test2.send(&Message::CallReject { caller: "test".into() })
        .unwrap();
    expect_success(test2.recv().unwrap());

    let rejected = recv_eventually(&mut test, Duration::from_secs(2));
    match rejected {
        Message::CallReject { caller } => assert_eq!(caller, "test2"),
        other => panic!("Esperaba CallReject y llegó: {:?}", other),
    }

    // registro "test3", reinicio server y hago login
    let mut test3 = connect_tls(&workdir);
    test3.set_read_timeout(Some(Duration::from_millis(200))).unwrap();

    test3.send(&Message::Register {
        username: "test3".into(),
        password: "pw3".into(),
        email: None,
    })
    .unwrap();
    expect_success(test3.recv().unwrap());

    // Mato server y lo reinicio
    drop(server);
    wait_for_port_close(Duration::from_secs(4));

    server = spawn_server(&workdir);
    wait_for_listen(Duration::from_secs(4));

    let mut test4 = connect_tls(&workdir);
    test4.set_read_timeout(Some(Duration::from_millis(200))).unwrap();

    test4.send(&Message::Login {
        username: "test3".into(),
        password: "pw3".into(),
    })
    .unwrap();
    expect_success(test4.recv().unwrap());

    test4.send(&Message::Login {
        username: "test".into(),
        password: "pw1".into(),
    })
    .unwrap();
    expect_success(test4.recv().unwrap());

    drop(server);
}
