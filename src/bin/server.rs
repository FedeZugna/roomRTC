use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use room_rtc::log::logger::Logger;

use rustls::pki_types::CertificateDer;
use rustls::{ServerConfig, ServerConnection, StreamOwned};
use std::sync::Arc as StdArc;
use std::sync::mpsc;

// ==================== TIPOS Y ESTRUCTURAS ====================

/// Estados posibles de un usuario
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum UserStatus {
    Disconnected,
    Available,
    Busy,
}

/// Información de un usuario registrado
#[derive(Debug, Clone)]
pub struct User {
    pub username: String,
    pub password: String,
    pub email: Option<String>,
}

/// Información de sesión activa de un usuario
#[derive(Debug, Clone)]
pub struct UserSession {
    pub username: String,
    pub status: UserStatus,
    pub peer_addr: String,
}

/// Información de una llamada activa
#[derive(Debug, Clone)]
pub struct Call {
    pub caller: String,
    pub callee: String,
}

/// Estado compartido del servidor
pub struct ServerState {
    /// Usuarios registrados (username -> User)
    pub users: HashMap<String, User>,

    /// Sesiones activas (username -> UserSession)
    pub sessions: HashMap<String, UserSession>,

    /// Llamadas activas
    pub calls: Vec<Call>,

    /// Límite de conexiones simultáneas
    pub max_connections: usize,

    /// Archivo de persistencia de usuarios
    pub users_file: String,

    /// guardamos un canal de salida para cada usuario.
    /// username -> Sender<Message>
    pub client_senders: HashMap<String, mpsc::Sender<Message>>, //

    /// Logger para registrar eventos del servidor
    pub logger: Logger,
}

/// Protocolo de mensajes entre cliente y servidor
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum Message {
    // Autenticación
    Register {
        username: String,
        password: String,
        email: Option<String>,
    },
    Login {
        username: String,
        password: String,
    },
    Logout,

    // Estados y listados
    GetUserList,
    UserListResponse {
        users: Vec<(String, UserStatus)>,
    },
    StatusUpdate {
        username: String,
        status: UserStatus,
    },

    // Llamadas
    CallRequest {
        callee: String,
        fingerprint: String,
    },
    CallIncoming {
        caller: String,
        fingerprint: String,
    },
    CallAccept {
        caller: String,
        fingerprint: String,
    },
    CallReject {
        caller: String,
    },
    CallEnd,

    // Señalización WebRTC (SDP y ICE candidates)
    Offer {
        sdp: String,
        target: String,
    },
    Answer {
        sdp: String,
        target: String,
    },
    IceCandidate {
        candidate: String,
        target: String,
    },

    // Respuestas
    Success {
        message: String,
    },
    Error {
        message: String,
    },
}

// ==================== IMPLEMENTACIÓN DEL SERVIDOR ====================

impl ServerState {
    /// Crea un nuevo estado del servidor
    pub fn new(users_file: String, max_connections: usize) -> Self {
        let logger = Logger::new("SERVER");
        logger.info("Inicializando servidor", "ServerState");

        let users = Self::load_users(&users_file);
        logger.info(
            &format!("Cargados {} usuarios desde archivo", users.len()),
            "ServerState",
        );

        ServerState {
            users,
            sessions: HashMap::new(),
            calls: Vec::new(),
            max_connections,
            users_file,
            client_senders: HashMap::new(), // *** CAMBIO
            logger,
        }
    }

    /// Carga los usuarios desde el archivo
    fn load_users(filename: &str) -> HashMap<String, User> {
        let mut users = HashMap::new();

        if let Ok(file) = File::open(filename) {
            let reader = BufReader::new(file);

            for line in reader.lines().map_while(Result::ok) {
                let parts: Vec<&str> = line.split('|').collect();
                if parts.len() >= 2 {
                    let user = User {
                        username: parts[0].to_string(),
                        password: parts[1].to_string(),
                        email: parts.get(2).map(|s| s.to_string()),
                    };
                    users.insert(user.username.clone(), user);
                }
            }
        }

        users
    }

    /// Guarda un usuario en el archivo
    fn save_user(&self, user: &User) -> std::io::Result<()> {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.users_file)?;

        let line = format!(
            "{}|{}|{}\n",
            user.username,
            user.password,
            user.email.as_deref().unwrap_or("")
        );

        file.write_all(line.as_bytes())?;
        Ok(())
    }

    /// Registra un nuevo usuario
    pub fn register_user(
        &mut self,
        username: String,
        password: String,
        email: Option<String>,
    ) -> Result<(), String> {
        if self.users.contains_key(&username) {
            self.logger.warn(
                &format!("Intento de registro con usuario existente: {}", username),
                "register_user",
            );
            return Err("Usuario ya existe".to_string());
        }

        let user = User {
            username: username.clone(),
            password,
            email,
        };

        self.save_user(&user).map_err(|e| {
            self.logger.error(
                &format!("Error guardando usuario {}: {}", username, e),
                "register_user",
            );
            format!("Error al guardar usuario: {}", e)
        })?;

        self.logger.info(
            &format!("Usuario registrado: {}", username),
            "register_user",
        );
        self.users.insert(username, user);
        Ok(())
    }

    /// Inicia sesión de un usuario
    pub fn login(
        &mut self,
        username: String,
        password: String,
        peer_addr: String,
    ) -> Result<(), String> {
        // Verificar que el usuario existe
        let user = self.users.get(&username).ok_or_else(|| {
            self.logger.warn(
                &format!("Intento de login con usuario no existente: {}", username),
                "login",
            );
            "Usuario no encontrado".to_string()
        })?;

        // Verificar contraseña
        if user.password != password {
            self.logger.warn(
                &format!("Contraseña incorrecta para usuario: {}", username),
                "login",
            );
            return Err("Contraseña incorrecta".to_string());
        }

        // Verificar que no esté ya conectado
        if self.sessions.contains_key(&username) {
            self.logger
                .warn(&format!("Usuario {} ya está conectado", username), "login");
            return Err("Usuario ya está conectado".to_string());
        }

        // Verificar límite de conexiones
        if self.sessions.len() >= self.max_connections {
            self.logger.warn(
                &format!("Servidor lleno. Rechazando login de {}", username),
                "login",
            );
            return Err("Servidor lleno".to_string());
        }

        // Crear sesión
        let session = UserSession {
            username: username.clone(),
            status: UserStatus::Available,
            peer_addr: peer_addr.clone(),
        };

        self.logger.info(
            &format!("Usuario {} inició sesión desde {}", username, peer_addr),
            "login",
        );
        self.sessions.insert(username, session);
        Ok(())
    }

    /// Cierra sesión de un usuario
    pub fn logout(&mut self, username: &str) {
        self.logger
            .info(&format!("Usuario {} cerró sesión", username), "logout");
        self.sessions.remove(username);
        self.client_senders.remove(username); // *** CAMBIO

        // Terminar llamadas activas
        let calls_before = self.calls.len();
        self.calls
            .retain(|call| call.caller != username && call.callee != username);
        let calls_after = self.calls.len();

        if calls_before != calls_after {
            self.logger.info(
                &format!(
                    "Terminadas {} llamadas por logout de {}",
                    calls_before - calls_after,
                    username
                ),
                "logout",
            );
        }
    }

    /// Obtiene el listado de usuarios con sus estados
    pub fn get_user_list(&self) -> Vec<(String, UserStatus)> {
        self.users
            .keys()
            .map(|username| {
                let status = self
                    .sessions
                    .get(username)
                    .map(|s| s.status.clone())
                    .unwrap_or(UserStatus::Disconnected);

                (username.clone(), status)
            })
            .collect()
    }

    /// Inicia una llamada (sólo estado; no envía mensajes)
    pub fn start_call(&mut self, caller: &str, callee: &str) -> Result<(), String> {
        // Verificar que ambos usuarios estén disponibles
        let caller_session = self
            .sessions
            .get(caller)
            .ok_or("Caller no está conectado")?;

        if caller_session.status != UserStatus::Available {
            self.logger.warn(
                &format!("Intento de llamada con caller {} no disponible", caller),
                "start_call",
            );
            return Err("Caller no está disponible".to_string());
        }

        let callee_session = self
            .sessions
            .get(callee)
            .ok_or("Callee no está conectado")?;

        if callee_session.status != UserStatus::Available {
            self.logger.warn(
                &format!("Intento de llamada con callee {} no disponible", callee),
                "start_call",
            );
            return Err("Callee no está disponible".to_string());
        }

        // Cambiar estados a Ocupado
        if let Some(session) = self.sessions.get_mut(caller) {
            session.status = UserStatus::Busy;
        }

        if let Some(session) = self.sessions.get_mut(callee) {
            session.status = UserStatus::Busy;
        }

        // Registrar llamada
        self.calls.push(Call {
            caller: caller.to_string(),
            callee: callee.to_string(),
        });

        self.logger.info(
            &format!("Iniciada llamada: {} -> {}", caller, callee),
            "start_call",
        );
        Ok(())
    }

    /// Termina una llamada y retorna el nombre del otro participante si existe
    pub fn end_call(&mut self, username: &str) -> Option<String> {
        // Encontrar y eliminar la llamada
        if let Some(index) = self
            .calls
            .iter()
            .position(|call| call.caller == username || call.callee == username)
        {
            let call = self.calls.remove(index);

            // Determinar quién es el otro participante
            let other_user = if call.caller == username {
                Some(call.callee.clone())
            } else {
                Some(call.caller.clone())
            };

            // Cambiar estados a Disponible
            if let Some(session) = self.sessions.get_mut(&call.caller) {
                session.status = UserStatus::Available;
            }

            if let Some(session) = self.sessions.get_mut(&call.callee) {
                session.status = UserStatus::Available;
            }

            self.logger.info(
                &format!("Finalizada llamada: {} <-> {}", call.caller, call.callee),
                "end_call",
            );
            other_user
        } else {
            self.logger.warn(
                &format!(
                    "Intento de terminar llamada inexistente para usuario {}",
                    username
                ),
                "end_call",
            );
            None
        }
    }
}

// ==================== SERVIDOR PRINCIPAL ====================

pub struct SignalingServer {
    state: Arc<Mutex<ServerState>>,
    listener: TcpListener,
    tls_config: StdArc<ServerConfig>,
}

impl SignalingServer {
    /// Crea un nuevo servidor con TLS
    pub fn new(
        addr: &str,
        users_file: String,
        max_connections: usize,
        cert_path: &str,
        key_path: &str,
    ) -> std::io::Result<Self> {
        let listener = TcpListener::bind(addr)?;
        let state = Arc::new(Mutex::new(ServerState::new(users_file, max_connections)));

        // Cargar certificados y clave privada
        let tls_config = load_tls_config(cert_path, key_path)?;

        Ok(SignalingServer {
            state,
            listener,
            tls_config,
        })
    }

    /// Inicia el servidor
    pub fn run(&self) -> std::io::Result<()> {
        let addr = self.listener.local_addr()?;
        println!("Servidor TLS iniciado en: {}", addr);

        {
            let state = self.state.lock().unwrap();
            state
                .logger
                .info(&format!("Servidor escuchando en {}", addr), "run");
        }

        for stream in self.listener.incoming() {
            match stream {
                Ok(stream) => {
                    let state = Arc::clone(&self.state);
                    let tls_config = StdArc::clone(&self.tls_config);

                    thread::spawn(move || {
                        if let Err(e) = handle_client_tls(stream, state, tls_config) {
                            eprintln!("Error manejando cliente: {}", e);
                        }
                    });
                }
                Err(e) => {
                    eprintln!("Error aceptando conexión: {}", e);
                    let state = self.state.lock().unwrap();
                    state
                        .logger
                        .error(&format!("Error aceptando conexión: {}", e), "run");
                }
            }
        }

        Ok(())
    }
}

// ==================== CONFIGURACIÓN TLS ====================

/// Carga la configuración TLS desde archivos de certificado y clave
fn load_tls_config(cert_path: &str, key_path: &str) -> std::io::Result<StdArc<ServerConfig>> {
    // Cargar certificados
    let cert_file = File::open(cert_path).map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("No se pudo abrir el certificado: {}", e),
        )
    })?;

    let mut cert_reader = BufReader::new(cert_file);
    let certs: Vec<CertificateDer> = rustls_pemfile::certs(&mut cert_reader)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Error leyendo certificados: {}", e),
            )
        })?;

    if certs.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "No se encontraron certificados en el archivo",
        ));
    }

    // Cargar clave privada
    let key_file = File::open(key_path).map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("No se pudo abrir la clave privada: {}", e),
        )
    })?;

    let mut key_reader = BufReader::new(key_file);
    let key = rustls_pemfile::private_key(&mut key_reader)
        .map_err(|e| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Error leyendo clave privada: {}", e),
            )
        })?
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "No se encontró clave privada en el archivo",
            )
        })?;

    // Configurar el servidor TLS
    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|e| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Error configurando TLS: {}", e),
            )
        })?;

    Ok(StdArc::new(config))
}

// ==================== MANEJO DE CLIENTES CON TLS ====================

fn handle_client_tls(
    stream: TcpStream,
    state: Arc<Mutex<ServerState>>,
    tls_config: StdArc<ServerConfig>,
) -> std::io::Result<()> {
    let peer_addr = stream.peer_addr()?.to_string();
    println!("Nueva conexión TLS desde: {}", peer_addr);

    {
        let st = state.lock().unwrap();
        st.logger.info(
            &format!("Nueva conexión TLS desde: {}", peer_addr),
            "handle_client_tls",
        );
    }

    // Crear conexión TLS
    let conn = ServerConnection::new(tls_config)
        .map_err(|e| std::io::Error::other(format!("Error creando conexión TLS: {}", e)))?;

    let mut tls_stream = StreamOwned::new(conn, stream);

    // lectura de la red y lectura del canal mpsc.
    {
        let sock = tls_stream.get_mut();
        sock.set_read_timeout(Some(Duration::from_millis(200)))?;
    }

    // Canal para mensajes ENVIADOS hacia este cliente (CallIncoming, Offer, etc.)
    let (tx_to_client, rx_for_client) = mpsc::channel::<Message>(); // *** NUEVO

    // Variable para almacenar el username del cliente conectado
    let mut current_user: Option<String> = None;

    loop {
        //Primero, enviar todos los mensajes pendientes del canal al cliente
        //    (mensajes que vienen de otros usuarios / del servidor)
        while let Ok(msg) = rx_for_client.try_recv() {
            if let Err(e) = write_message(&mut tls_stream, &msg) {
                eprintln!("[Server] Error enviando msg a cliente {}: {}", peer_addr, e);
                // si falla escribir, cortamos conexión
                if let Some(u) = current_user.take() {
                    let mut st = state.lock().unwrap();
                    st.logout(&u);
                    println!("Usuario {} desconectado (error escritura)", u);
                }
                return Ok(());
            }
        }

        // Intentar leer un mensaje del cliente
        let msg_res = read_message(&mut tls_stream);
        match msg_res {
            Ok(message) => {
                // Procesar mensaje y obtener respuesta
                let response = process_message(
                    message,
                    &state,
                    &peer_addr,
                    &mut current_user,
                    &tx_to_client,
                );

                // Enviar respuesta inmediata al propio cliente
                if let Err(e) = write_message(&mut tls_stream, &response) {
                    eprintln!("Error enviando respuesta: {}", e);
                    break;
                }
            }
            Err(e) => {
                use std::io::ErrorKind;
                match e.kind() {
                    ErrorKind::WouldBlock | ErrorKind::TimedOut => {
                        // No llegó nada, seguimos el loop para volver a chequear el canal.
                        continue;
                    }
                    ErrorKind::UnexpectedEof => {
                        println!("Cliente desconectado: {}", peer_addr);
                    }
                    _ => {
                        eprintln!("Error leyendo mensaje: {}", e);
                    }
                }
                break;
            }
        }
    }

    // Cleanup: cerrar sesión si el usuario estaba conectado
    if let Some(username) = current_user {
        let mut state = state.lock().unwrap();
        state.logout(&username);
        println!("Usuario {} desconectado", username);
    } else {
        let state = state.lock().unwrap();
        state.logger.info(
            &format!("Conexión desde {} cerrada sin login", peer_addr),
            "handle_client_tls",
        );
    }

    Ok(())
}

/// Procesa un mensaje y retorna la respuesta apropiada
fn process_message(
    message: Message,
    state: &Arc<Mutex<ServerState>>,
    peer_addr: &str,
    current_user: &mut Option<String>,
    my_sender: &mpsc::Sender<Message>,
) -> Message {
    match message {
        Message::Register {
            username,
            password,
            email,
        } => {
            let mut state = state.lock().unwrap();
            match state.register_user(username, password, email) {
                Ok(_) => Message::Success {
                    message: "Usuario registrado exitosamente".to_string(),
                },
                Err(e) => Message::Error { message: e },
            }
        }

        Message::Login { username, password } => {
            let mut state = state.lock().unwrap();
            match state.login(username.clone(), password, peer_addr.to_string()) {
                Ok(_) => {
                    // *** NUEVO: asociar canal mpsc a este usuario
                    state
                        .client_senders
                        .insert(username.clone(), my_sender.clone());
                    *current_user = Some(username);
                    Message::Success {
                        message: "Login exitoso".to_string(),
                    }
                }
                Err(e) => Message::Error { message: e },
            }
        }

        Message::Logout => {
            if let Some(username) = current_user.take() {
                let mut state = state.lock().unwrap();
                state.logout(&username);
                Message::Success {
                    message: "Logout exitoso".to_string(),
                }
            } else {
                Message::Error {
                    message: "No hay sesión activa".to_string(),
                }
            }
        }

        Message::GetUserList => {
            let state = state.lock().unwrap();
            let users = state.get_user_list();
            Message::UserListResponse { users }
        }

        Message::CallRequest {
            callee,
            fingerprint,
        } => {
            // caller es el usuario actual
            let caller = match &current_user {
                Some(u) => u.clone(),
                None => {
                    return Message::Error {
                        message: "Debe iniciar sesión primero".to_string(),
                    };
                }
            };
            let mut st = state.lock().unwrap();

            // actualizar estado de llamada (verifica que ambos están disponibles)
            if let Err(e) = st.start_call(&caller, &callee) {
                return Message::Error { message: e };
            }

            // buscar canal del callee
            let target_tx = match st.client_senders.get(&callee) {
                Some(tx) => tx.clone(),
                None => {
                    // revertir la llamada si el callee no está conectado realmente
                    st.end_call(&caller);
                    return Message::Error {
                        message: format!("Usuario {} no está conectado", callee),
                    };
                }
            };

            // enviar CallIncoming al callee
            if let Err(e) = target_tx.send(Message::CallIncoming {
                caller: caller.clone(),
                fingerprint,
            }) {
                eprintln!("[Server] Error enviando CallIncoming a {}: {}", callee, e);
                // revertir
                st.end_call(&caller);
                return Message::Error {
                    message: "Callee no disponible".to_string(),
                };
            }

            Message::Success {
                message: format!("Llamada enviada a {}", callee),
            }
        }

        Message::CallReject { caller } => {
            // este mensaje lo manda el callee (quien recibió la llamada)
            let me = match &current_user {
                Some(u) => u.clone(),
                None => {
                    return Message::Error {
                        message: "No hay sesión activa".to_string(),
                    };
                }
            };

            let mut st = state.lock().unwrap();

            // sacar la llamada activa si existe
            if let Some(idx) = st
                .calls
                .iter()
                .position(|c| c.caller == caller && c.callee == me)
            {
                st.calls.remove(idx);
            }

            // poner a ambos como Available
            if let Some(sess) = st.sessions.get_mut(&caller) {
                sess.status = UserStatus::Available;
            }
            if let Some(sess) = st.sessions.get_mut(&me) {
                sess.status = UserStatus::Available;
            }

            // notificar al caller, si está conectado
            if let Some(tx_caller) = st.client_senders.get(&caller) {
                let _ = tx_caller.send(Message::CallReject { caller: me.clone() });
            }

            Message::Success {
                message: "Llamada rechazada".to_string(),
            }
        }

        Message::CallAccept {
            caller,
            fingerprint,
        } => {
            // este mensaje lo manda el callee (quien recibió la llamada) para aceptarla
            let me = match &current_user {
                Some(u) => u.clone(),
                None => {
                    return Message::Error {
                        message: "No hay sesión activa".to_string(),
                    };
                }
            };

            let st = state.lock().unwrap();

            // Verificar que existe una llamada pendiente
            let call_exists = st
                .calls
                .iter()
                .any(|c| c.caller == caller && c.callee == me);

            if !call_exists {
                return Message::Error {
                    message: "No hay llamada pendiente".to_string(),
                };
            }

            // notificar al caller que se aceptó la llamada
            if let Some(tx_caller) = st.client_senders.get(&caller) {
                if let Err(e) = tx_caller.send(Message::CallAccept {
                    caller: me.clone(),
                    fingerprint,
                }) {
                    eprintln!("[Server] Error enviando CallAccept a {}: {}", caller, e);
                    return Message::Error {
                        message: "No se pudo notificar al caller".to_string(),
                    };
                }
            } else {
                return Message::Error {
                    message: format!("Usuario {} no está conectado", caller),
                };
            }

            Message::Success {
                message: "Llamada aceptada".to_string(),
            }
        }

        Message::CallEnd => {
            if let Some(username) = current_user {
                let mut state = state.lock().unwrap();

                // end_call retorna el nombre del otro participante
                if let Some(other_user) = state.end_call(username) {
                    // Notificar al otro participante que la llamada terminó
                    if let Some(tx_other) = state.client_senders.get(&other_user) {
                        let _ = tx_other.send(Message::CallEnd);
                        println!(
                            "[Server] Notificando a {} que la llamada terminó",
                            other_user
                        );
                    }
                }

                Message::Success {
                    message: "Llamada finalizada".to_string(),
                }
            } else {
                Message::Error {
                    message: "No hay sesión activa".to_string(),
                }
            }
        }

        // Estos mensajes requieren relay a otro cliente
        Message::Offer { sdp, target } => {
            if let Some(sender) = current_user.clone() {
                relay_signaling(
                    Message::Offer {
                        sdp,
                        target: sender.clone(), // el target verá quién le mandó
                    },
                    &target,
                    state,
                )
            } else {
                Message::Error {
                    message: "Debe iniciar sesion primero".to_string(),
                }
            }
        }

        Message::Answer { sdp, target } => {
            if let Some(sender) = current_user.clone() {
                relay_signaling(
                    Message::Answer {
                        sdp,
                        target: sender.clone(),
                    },
                    &target,
                    state,
                )
            } else {
                Message::Error {
                    message: "Debe iniciar sesion primero".to_string(),
                }
            }
        }

        Message::IceCandidate { candidate, target } => {
            if let Some(sender) = current_user.clone() {
                relay_signaling(
                    Message::IceCandidate {
                        candidate,
                        target: sender.clone(),
                    },
                    &target,
                    state,
                )
            } else {
                Message::Error {
                    message: "Debe iniciar sesion primero".to_string(),
                }
            }
        }

        _ => Message::Error {
            message: "Mensaje no soportado".to_string(),
        },
    }
}

/// Relay genérico para Offer / Answer / IceCandidate
fn relay_signaling(
    msg_to_target: Message,
    target_username: &str,
    state: &Arc<Mutex<ServerState>>,
) -> Message {
    // Buscar el sender del target
    let target_tx = {
        let state = state.lock().unwrap();
        state.client_senders.get(target_username).cloned()
    };

    if let Some(tx) = target_tx {
        if let Err(e) = tx.send(msg_to_target) {
            eprintln!(
                "[Server] Error relay signaling a {}: {}",
                target_username, e
            );
            Message::Error {
                message: "Fallo al reenviar mensaje de señalización".to_string(),
            }
        } else {
            Message::Success {
                message: format!("Mensaje de señalización enviado a {}", target_username),
            }
        }
    } else {
        Message::Error {
            message: format!("Usuario destino {} no está conectado", target_username),
        }
    }
}

// ==================== FUNCIONES DE AYUDA ====================

// Estas funciones deberías implementarlas según tu protocolo.
// Si lo queremos cambiar al protocolo, tenemos que cambiar esta función
fn read_message<S: Read>(stream: &mut S) -> std::io::Result<Message> {
    // Implementar lectura de mensaje según tu protocolo
    // Por ejemplo: longitud (4 bytes) + JSON

    // Leer longitud del mensaje (4 bytes, big-endian)
    let mut len_bytes = [0u8; 4];
    stream.read_exact(&mut len_bytes)?;
    let msg_len = u32::from_be_bytes(len_bytes) as usize;

    // Validar longitud razonable (por ejemplo, máximo 1MB)
    if msg_len > 1_000_000 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Mensaje demasiado grande",
        ));
    }

    // Leer el mensaje
    let mut buffer = vec![0u8; msg_len];
    stream.read_exact(&mut buffer)?;

    // Deserializar desde JSON
    serde_json::from_slice(&buffer).map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("Error deserializando mensaje: {}", e),
        )
    })
}

fn write_message<S: Write>(stream: &mut S, message: &Message) -> std::io::Result<()> {
    // Serializar a JSON
    let json = serde_json::to_vec(message).map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("Error serializando mensaje: {}", e),
        )
    })?;

    // Escribir longitud (4 bytes, big-endian)
    let len_bytes = (json.len() as u32).to_be_bytes();
    stream.write_all(&len_bytes)?;

    // Escribir mensaje
    stream.write_all(&json)?;
    stream.flush()?;

    Ok(())
}

fn main() -> std::io::Result<()> {
    let server = SignalingServer::new(
        "127.0.0.1:8443",            // Dirección y puerto
        "users.txt".to_string(),     // Archivo de usuarios
        100,                         // Máximo de conexiones
        "certs/localhost+2.pem",     // Certificado
        "certs/localhost+2-key.pem", // Clave privada
    )?;

    server.run()
}

// ==================== TESTS ====================

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Helper para asegurar que arrancamos sin un archivo sucio de una ejecución anterior
    fn clean_env(filename: &str) {
        let _ = fs::remove_file(filename);
    }

    #[test]
    fn test_register_user() {
        let filename = "test_users_register.txt";
        clean_env(filename); // Limpieza inicial

        let mut state = ServerState::new(filename.to_string(), 10);

        let result = state.register_user(
            "test_user".to_string(),
            "password123".to_string(),
            Some("test@example.com".to_string()),
        );

        assert!(result.is_ok(), "El registro debería ser exitoso");
        assert!(state.users.contains_key("test_user"));

        clean_env(filename); // Limpieza final
    }

    #[test]
    fn test_duplicate_login() {
        let filename = "test_users_login.txt";
        clean_env(filename);

        let mut state = ServerState::new(filename.to_string(), 10);

        // Registrar usuario
        state
            .register_user("user1".to_string(), "pass".to_string(), None)
            .expect("Fallo al registrar usuario");

        // Primer Login
        state
            .login(
                "user1".to_string(),
                "pass".to_string(),
                "127.0.0.1".to_string(),
            )
            .expect("Fallo el primer login");

        // Segundo Login (Debe fallar porque ya está conectado)
        let result = state.login(
            "user1".to_string(),
            "pass".to_string(),
            "127.0.0.1".to_string(),
        );

        assert!(result.is_err(), "El segundo login debería haber fallado");
        assert_eq!(result.unwrap_err(), "Usuario ya está conectado");

        clean_env(filename);
    }

    #[test]
    fn test_call_flow() {
        let filename = "test_users_call.txt";
        clean_env(filename);

        let mut state = ServerState::new(filename.to_string(), 10);

        // Registrar usuarios
        state
            .register_user("alice".to_string(), "pass1".to_string(), None)
            .unwrap();
        state
            .register_user("bob".to_string(), "pass2".to_string(), None)
            .unwrap();

        // Login
        state
            .login(
                "alice".to_string(),
                "pass1".to_string(),
                "127.0.0.1".to_string(),
            )
            .unwrap();
        state
            .login(
                "bob".to_string(),
                "pass2".to_string(),
                "127.0.0.2".to_string(),
            )
            .unwrap();

        // Verifico estado inicial
        assert_eq!(
            state.sessions.get("alice").unwrap().status,
            UserStatus::Available
        );
        assert_eq!(
            state.sessions.get("bob").unwrap().status,
            UserStatus::Available
        );

        // Inicio llamada
        let result = state.start_call("alice", "bob");
        assert!(result.is_ok(), "El inicio de llamada falló");

        // Verifico estados (Deben estar Busy)
        assert_eq!(
            state.sessions.get("alice").unwrap().status,
            UserStatus::Busy
        );
        assert_eq!(state.sessions.get("bob").unwrap().status, UserStatus::Busy);

        // Verifico que la llamada existe en la lista
        assert!(
            state
                .calls
                .iter()
                .any(|c| c.caller == "alice" && c.callee == "bob")
        );

        // Termino llamada
        let other_user = state.end_call("alice");
        assert_eq!(other_user, Some("bob".to_string()));

        // Verifico estados después (Deben estar Available)
        assert_eq!(
            state.sessions.get("alice").unwrap().status,
            UserStatus::Available
        );
        assert_eq!(
            state.sessions.get("bob").unwrap().status,
            UserStatus::Available
        );

        // Verifico que la lista de llamadas está vacía
        assert!(state.calls.is_empty());

        clean_env(filename);
    }
}
