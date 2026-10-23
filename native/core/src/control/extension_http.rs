//! Authenticated, profile-scoped HTTP transport for registered control extensions.
//! This listener never dispatches common control commands or owns domain state.
use super::*;
use std::fs::{File, OpenOptions};
use std::io::Read;
use std::net::{Ipv4Addr, SocketAddrV4, TcpListener, TcpStream};
use std::os::unix::fs::OpenOptionsExt;
use std::sync::atomic::AtomicUsize;

const DESCRIPTOR_FILE: &str = "extension-http.json";
const MAX_HEADER_BYTES: usize = 8192;
const MAX_DESCRIPTOR_BYTES: u64 = 4096;
// Unauthenticated network peers must never consume private Unix control slots.
const MAX_HTTP_CONNECTIONS: usize = 8;

#[derive(Serialize)]
struct Descriptor {
    version: u64,
    instance_id: String,
    config_dir: PathBuf,
    state_dir: PathBuf,
    url: String,
    token: String,
}

pub(super) struct ExtensionHttp {
    listener: Option<TcpListener>,
    accept_thread: Option<JoinHandle<()>>,
    descriptor: Descriptor,
    descriptor_bytes: Vec<u8>,
    descriptor_path: PathBuf,
    stop: Arc<AtomicBool>,
}

impl ExtensionHttp {
    pub(super) fn bind(inner: &ControlInner) -> Result<Self, String> {
        // Snapshot runtime identity under its lock; no filesystem or network I/O
        // occurs while holding the domain lock.
        let instance_id = {
            let state = lock_unpoisoned(&inner.shared);
            let context = state
                .update_context()
                .ok_or("daemon instance unavailable")?;
            let profile = state.profile_store().ok_or("daemon profile unavailable")?;
            if context.config_dir != inner.paths.config_dir
                || context.state_dir != inner.paths.state_dir
                || profile.config_dir() != inner.paths.config_dir
                || profile.lifecycle_lock().state_dir() != inner.paths.state_dir
                || context.instance_id.is_empty()
                || context.instance_id.len() > 128
                || !context
                    .instance_id
                    .bytes()
                    .all(|byte| byte.is_ascii_graphic())
            {
                return Err("extension HTTP runtime profile or instance mismatch".into());
            }
            context.instance_id.clone()
        };
        let listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0))
            .map_err(|error| format!("cannot bind extension HTTP listener: {error}"))?;
        listener
            .set_nonblocking(true)
            .map_err(|error| error.to_string())?;
        let address = listener.local_addr().map_err(|error| error.to_string())?;
        let mut random = [0u8; 32];
        File::open("/dev/urandom")
            .and_then(|mut file| file.read_exact(&mut random))
            .map_err(|error| format!("cannot generate extension HTTP token: {error}"))?;
        let mut token = String::with_capacity(64);
        use std::fmt::Write as _;
        for byte in random {
            write!(&mut token, "{byte:02x}").expect("writing to String");
        }
        let descriptor = Descriptor {
            version: 1,
            instance_id,
            config_dir: inner.paths.config_dir.clone(),
            state_dir: inner.paths.state_dir.clone(),
            url: format!("http://{address}/extension"),
            token,
        };
        let descriptor_bytes =
            serde_json::to_vec(&descriptor).map_err(|error| error.to_string())?;
        if descriptor_bytes.len() as u64 > MAX_DESCRIPTOR_BYTES {
            return Err("extension HTTP descriptor exceeds discovery byte limit".into());
        }
        Ok(Self {
            listener: Some(listener),
            accept_thread: None,
            descriptor,
            descriptor_bytes,
            descriptor_path: inner.paths.state_dir.join(DESCRIPTOR_FILE),
            stop: Arc::new(AtomicBool::new(false)),
        })
    }

    pub(super) fn start(&mut self, inner: Arc<ControlInner>) -> Result<(), String> {
        let listener = self
            .listener
            .take()
            .ok_or("extension HTTP listener unavailable")?;
        let host = listener
            .local_addr()
            .map_err(|error| error.to_string())?
            .to_string();
        // The profile store's private atomic writer preserves 0600 and rejects
        // symlinks, nonowned files, and unsafe state directories.
        if let Err(error) = desktop_pet_contracts::profile_store::atomic_write(
            &self.descriptor_path,
            &self.descriptor_bytes,
        ) {
            // atomic_write can fail at directory fsync after a successful rename.
            self.retire_descriptor();
            return Err(error);
        }
        let stop = Arc::clone(&self.stop);
        let token = self.descriptor.token.clone();
        match thread::Builder::new()
            .name("desktop-pet-extension-http".into())
            .spawn(move || accept_http(listener, inner, stop, token, host))
        {
            Ok(handle) => self.accept_thread = Some(handle),
            Err(error) => {
                self.retire_descriptor();
                return Err(format!("cannot start extension HTTP listener: {error}"));
            }
        }
        Ok(())
    }

    pub(super) fn shutdown(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.listener.take();
        if let Some(handle) = self.accept_thread.take() {
            let _ = handle.join();
        }
        self.retire_descriptor();
    }

    fn retire_descriptor(&self) {
        // Never remove a replacement daemon's descriptor (or a symlink). Read
        // only a bounded, private, owned regular file and compare this lifetime.
        let Ok(mut file) = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(&self.descriptor_path)
        else {
            return;
        };
        let Ok(metadata) = file.metadata() else {
            return;
        };
        if !metadata.is_file()
            || metadata.uid() != unsafe { libc::geteuid() }
            || metadata.mode() & 0o777 != 0o600
            || metadata.len() > MAX_DESCRIPTOR_BYTES
        {
            return;
        }
        let mut bytes = Vec::new();
        if (&mut file)
            .take(MAX_DESCRIPTOR_BYTES + 1)
            .read_to_end(&mut bytes)
            .is_err()
            || bytes != self.descriptor_bytes
        {
            return;
        }
        if fs::symlink_metadata(&self.descriptor_path)
            .is_ok_and(|current| current.dev() == metadata.dev() && current.ino() == metadata.ino())
        {
            let _ = fs::remove_file(&self.descriptor_path);
        }
    }
}

impl Drop for ExtensionHttp {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn accept_http(
    listener: TcpListener,
    inner: Arc<ControlInner>,
    stop: Arc<AtomicBool>,
    token: String,
    host: String,
) {
    let token: Arc<str> = token.into();
    let host: Arc<str> = host.into();
    let active_http = Arc::new(AtomicUsize::new(0));
    while !stop.load(Ordering::Acquire) && !inner.stopping.load(Ordering::Acquire) {
        reap_finished_clients(&inner);
        let (stream, peer) = match listener.accept() {
            Ok(connection) => connection,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => {
                thread::sleep(Duration::from_millis(10));
                continue;
            }
        };
        if peer.ip() != Ipv4Addr::LOCALHOST
            || stop.load(Ordering::Acquire)
            || inner.stopping.load(Ordering::Acquire)
            || stream.set_nonblocking(false).is_err()
            || active_http
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |active| {
                    (active < MAX_HTTP_CONNECTIONS).then_some(active + 1)
                })
                .is_err()
        {
            continue;
        }
        let client_inner = Arc::clone(&inner);
        let token = Arc::clone(&token);
        let host = Arc::clone(&host);
        let client_active = Arc::clone(&active_http);
        let deadline = Instant::now() + CLIENT_TIMEOUT;
        let handle = thread::Builder::new()
            .name("desktop-pet-extension-http-client".into())
            .spawn(move || {
                handle_http(stream, &client_inner, &token, &host, deadline);
                client_active.fetch_sub(1, Ordering::Relaxed);
            });
        match handle {
            Ok(handle) => lock_unpoisoned(&inner.clients).push(handle),
            Err(_) => {
                active_http.fetch_sub(1, Ordering::Relaxed);
            }
        }
    }
}

fn handle_http(
    mut stream: TcpStream,
    inner: &ControlInner,
    token: &str,
    host: &str,
    deadline: Instant,
) {
    let result = read_request(&mut stream, inner, token, host, deadline).and_then(|body| {
        reject_duplicate_json_keys(&body).map_err(|_| 400u16)?;
        let kind: FrameKind = serde_json::from_slice(&body).map_err(|_| 400u16)?;
        let extension = inner
            .extensions
            .iter()
            .find(|extension| Some(extension.kind()) == kind.kind.as_deref())
            .ok_or(400u16)?;
        dispatch_extension(inner, extension.as_ref(), &body, body.len() + 1, deadline)
            .map_err(|_| 400u16)
    });
    let (status, body) = match result {
        Ok(body) => (200, body),
        Err(status) => (
            status,
            b"{\"error\":\"extension HTTP request rejected\"}\n".to_vec(),
        ),
    };
    let header = format!(
        "HTTP/1.1 {status} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\n\r\n",
        if status == 200 { "OK" } else { "Rejected" }, body.len()
    );
    let _ = write_http(&mut stream, header.as_bytes(), deadline)
        .and_then(|()| write_http(&mut stream, &body, deadline));
}

fn read_http(stream: &mut TcpStream, buffer: &mut [u8], deadline: Instant) -> Result<usize, u16> {
    stream
        .set_read_timeout(Some(socket::remaining(deadline).map_err(|_| 408u16)?))
        .map_err(|_| 400u16)?;
    loop {
        match stream.read(buffer) {
            Ok(0) => return Err(400),
            Ok(count) => return Ok(count),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {
                stream
                    .set_read_timeout(Some(socket::remaining(deadline).map_err(|_| 408u16)?))
                    .map_err(|_| 400u16)?;
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
                ) =>
            {
                return Err(408)
            }
            Err(_) => return Err(400),
        }
    }
}

fn read_request(
    stream: &mut TcpStream,
    inner: &ControlInner,
    token: &str,
    host: &str,
    deadline: Instant,
) -> Result<Vec<u8>, u16> {
    let mut header = Vec::with_capacity(1024);
    let mut chunk = [0u8; 1024];
    let header_end = loop {
        let available = (MAX_HEADER_BYTES - header.len()).min(chunk.len());
        if available == 0 {
            return Err(431);
        }
        let count = read_http(stream, &mut chunk[..available], deadline)?;
        header.extend_from_slice(&chunk[..count]);
        if let Some(end) = header.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
            break end + 4;
        }
    };
    let text = std::str::from_utf8(&header[..header_end - 4]).map_err(|_| 400u16)?;
    let mut lines = text.split("\r\n");
    if lines.next() != Some("POST /extension HTTP/1.1") {
        return Err(400);
    }
    let mut seen = HashSet::new();
    let mut content_length = None;
    let mut authenticated = false;
    let mut valid_host = false;
    let mut content_type = false;
    for line in lines {
        let (name, value) = line.split_once(':').ok_or(400u16)?;
        if name.is_empty() || !name.bytes().all(is_header_token) {
            return Err(400);
        }
        let name = name.to_ascii_lowercase();
        if value
            .bytes()
            .any(|byte| !(byte == b'\t' || (0x20..=0x7e).contains(&byte)))
        {
            return Err(400);
        }
        let value = value.trim_matches([' ', '\t']);
        match name.as_str() {
            "origin" | "transfer-encoding" | "expect" | "trailer" | "upgrade" => return Err(400),
            "host" => valid_host = value == host,
            "authorization" => {
                let bearer = value.strip_prefix("Bearer ").ok_or(401u16)?;
                authenticated = bearer.len() == token.len()
                    && bearer
                        .bytes()
                        .zip(token.bytes())
                        .fold(0u8, |diff, (left, right)| diff | (left ^ right))
                        == 0;
            }
            "content-type" => content_type = value.eq_ignore_ascii_case("application/json"),
            "content-length" => {
                if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
                    return Err(400);
                }
                content_length = Some(value.parse::<usize>().map_err(|_| 413u16)?);
            }
            "connection"
                if !value.eq_ignore_ascii_case("close")
                    && !value.eq_ignore_ascii_case("keep-alive") =>
            {
                return Err(400)
            }
            _ => {}
        }
        if !seen.insert(name) {
            return Err(400);
        }
    }
    if !authenticated {
        return Err(401);
    }
    if !valid_host || !content_type {
        return Err(400);
    }
    let content_length = content_length.ok_or(400u16)?;
    let limit = inner
        .extensions
        .iter()
        .map(|extension| extension.max_request_bytes())
        .max()
        .unwrap_or(0);
    if content_length == 0 || content_length >= limit {
        return Err(413);
    }
    if header.len() - header_end > content_length {
        return Err(400);
    }
    let mut body = Vec::with_capacity(content_length);
    body.extend_from_slice(&header[header_end..]);
    while body.len() < content_length {
        let count = read_http(
            stream,
            &mut chunk[..(content_length - body.len()).min(1024)],
            deadline,
        )?;
        body.extend_from_slice(&chunk[..count]);
    }
    Ok(body)
}

fn is_header_token(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte)
}

fn write_http(stream: &mut TcpStream, mut bytes: &[u8], deadline: Instant) -> io::Result<()> {
    while !bytes.is_empty() {
        stream.set_write_timeout(Some(socket::remaining(deadline)?))?;
        match stream.write(bytes) {
            Ok(0) => return Err(io::Error::new(io::ErrorKind::WriteZero, "HTTP peer closed")),
            Ok(count) => bytes = &bytes[count..],
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use desktop_pet_contracts::{PreferenceAccess, ProfileStore};
    use desktop_pet_update_coordinator::ExecutableIdentity;
    use std::sync::atomic::AtomicUsize;

    struct FixtureExtension {
        mutations: AtomicUsize,
        response: Vec<u8>,
    }

    impl ControlExtension for FixtureExtension {
        fn kind(&self) -> &'static str {
            "fixture"
        }
        fn max_request_bytes(&self) -> usize {
            256
        }
        fn max_response_bytes(&self) -> usize {
            128
        }
        fn handle_frame(&self, _: &[u8], _: &dyn PreferenceAccess) -> Result<Vec<u8>, String> {
            self.mutations.fetch_add(1, Ordering::SeqCst);
            Ok(self.response.clone())
        }
    }

    struct Fixture {
        _directory: tempfile::TempDir,
        server: ControlServer,
        extension: Arc<FixtureExtension>,
    }

    impl Fixture {
        fn new(response: &[u8]) -> Self {
            let directory = tempfile::tempdir_in("/tmp").unwrap();
            let root = fs::canonicalize(directory.path()).unwrap();
            let config_dir = root.join("config");
            let state_dir = root.join("state");
            for path in [&config_dir, &state_dir] {
                fs::create_dir(path).unwrap();
                fs::set_permissions(path, Permissions::from_mode(0o700)).unwrap();
            }
            let profile = ProfileStore::acquire(&config_dir, &state_dir).unwrap();
            let paths = Paths {
                config_dir: config_dir.clone(),
                state_dir: state_dir.clone(),
                control_socket: state_dir.join("control.sock"),
                log_file: state_dir.join("daemon.log"),
            };
            let mut state = AppState::new();
            state.set_lifecycle_extensions(Vec::new(), profile);
            state
                .set_update_context(UpdateContext {
                    executable: root.join("desktop-pet"),
                    version: "test".into(),
                    instance_id: "daemon-http-1".into(),
                    config_dir: config_dir.clone(),
                    #[cfg(desktop_pet_legacy)]
                    host_plugin_config_dir: None,
                    #[cfg(desktop_pet_legacy)]
                    herdr_socket: state_dir.join("host.sock"),
                    state_dir,
                    running: ExecutableIdentity {
                        path: root.join("desktop-pet"),
                        sha256: "a".repeat(64),
                    },
                    running_origin: None,
                    assets_override: None,
                    environment: Default::default(),
                    locale: "en".into(),
                })
                .unwrap();
            let extension = Arc::new(FixtureExtension {
                mutations: AtomicUsize::new(0),
                response: response.to_vec(),
            });
            let mut server = ControlServer::bind(
                Arc::new(Mutex::new(state)),
                paths,
                root.join("desktop-pet"),
                PackService::new(config_dir, root.join("assets"), None),
                vec![extension.clone()],
                Vec::new(),
            )
            .unwrap();
            assert!(!server.inner.paths.state_dir.join(DESCRIPTOR_FILE).exists());
            server.start().unwrap();
            Self {
                _directory: directory,
                server,
                extension,
            }
        }

        fn descriptor(&self) -> Value {
            serde_json::from_slice(
                &fs::read(self.server.inner.paths.state_dir.join(DESCRIPTOR_FILE)).unwrap(),
            )
            .unwrap()
        }

        fn host(&self) -> String {
            self.descriptor()["url"]
                .as_str()
                .unwrap()
                .strip_prefix("http://")
                .unwrap()
                .strip_suffix("/extension")
                .unwrap()
                .to_owned()
        }

        fn token(&self) -> String {
            self.descriptor()["token"].as_str().unwrap().to_owned()
        }

        fn headers(&self, length: usize) -> String {
            format!("POST /extension HTTP/1.1\r\nHost: {}\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: {length}\r\n", self.host(), self.token())
        }

        fn request(&self, header: &str, body: &[u8]) -> Vec<u8> {
            let mut stream = TcpStream::connect(self.host()).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(4)))
                .unwrap();
            stream.write_all(header.as_bytes()).unwrap();
            stream.write_all(b"\r\n").unwrap();
            // Early header rejection may close before the body reaches the peer.
            let _ = stream.write_all(body);
            let mut response = Vec::new();
            match stream.read_to_end(&mut response) {
                Ok(_) => {}
                Err(error) if error.kind() == io::ErrorKind::ConnectionReset => {}
                Err(error) => panic!("HTTP read failed: {error}"),
            }
            response
        }

        fn status(response: &[u8]) -> u16 {
            std::str::from_utf8(response)
                .unwrap()
                .split_whitespace()
                .nth(1)
                .unwrap()
                .parse()
                .unwrap()
        }
    }

    const REPLY: &[u8] =
        b"{\"version\":2,\"kind\":\"fixture\",\"instance_id\":\"daemon-http-1\",\"ok\":true}\n";
    const REQUEST: &[u8] = b"{\"version\":2,\"kind\":\"fixture\"}";

    #[test]
    fn authenticated_http_and_private_socket_share_extension_handler() {
        let fixture = Fixture::new(REPLY);
        let descriptor = fixture.descriptor();
        assert_eq!(descriptor.as_object().unwrap().len(), 6);
        assert_eq!(descriptor["version"], 1);
        assert_eq!(descriptor["instance_id"], "daemon-http-1");
        assert_eq!(
            descriptor["config_dir"],
            fixture.server.inner.paths.config_dir.to_str().unwrap()
        );
        assert_eq!(
            descriptor["state_dir"],
            fixture.server.inner.paths.state_dir.to_str().unwrap()
        );
        let token = fixture.token();
        assert_eq!(token.len(), 64);
        assert!(token
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)));
        let metadata =
            fs::metadata(fixture.server.inner.paths.state_dir.join(DESCRIPTOR_FILE)).unwrap();
        assert_eq!(metadata.mode() & 0o777, 0o600);
        let response = fixture.request(&fixture.headers(REQUEST.len()), REQUEST);
        assert_eq!(Fixture::status(&response), 200);
        assert!(response.ends_with(REPLY));
        let mut stream = UnixStream::connect(&fixture.server.inner.paths.control_socket).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(4)))
            .unwrap();
        stream.write_all(REQUEST).unwrap();
        stream.write_all(b"\n").unwrap();
        let mut response = Vec::new();
        stream.read_to_end(&mut response).unwrap();
        assert_eq!(response, REPLY);
        assert_eq!(fixture.extension.mutations.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn http_rejects_unauthorized_browser_framing_and_duplicate_json_before_mutation() {
        let fixture = Fixture::new(REPLY);
        let good = fixture.headers(REQUEST.len());
        let invalid = [
            good.replace(
                &format!("Authorization: Bearer {}\r\n", fixture.token()),
                "",
            ),
            good.replace(&fixture.token(), &"0".repeat(64)),
            format!("{good}Origin: http://localhost\r\n"),
            format!("{good}origin:\r\n"),
            format!("{good}Transfer-Encoding: chunked\r\n"),
            format!("{good}content-length: {}\r\n", REQUEST.len()),
            format!("{good}Authorization: Bearer {}\r\n", fixture.token()),
            format!("{good}X-Duplicate: one\r\nx-duplicate: two\r\n"),
            format!("{good} Folded: value\r\n"),
            format!("{good}Bad Header: value\r\n"),
            good.replace("POST /extension", "GET /extension"),
            good.replace("/extension", "/stop"),
            good.replace(
                "POST /extension",
                &format!("POST http://{}/extension", fixture.host()),
            ),
            good.replace(&fixture.host(), "localhost:1"),
            good.replace(&fixture.host(), "127.0.0.1:1"),
            good.replace("Content-Length: ", "Content-Length: +"),
            good.replace("Content-Type: application/json\r\n", ""),
            format!("{good}Expect: 100-continue\r\n"),
            format!("{good}X-Bad: value\ninjected\r\n"),
        ];
        for header in invalid {
            assert_ne!(
                Fixture::status(&fixture.request(&header, REQUEST)),
                200,
                "{header:?}"
            );
        }
        for body in [
            br#"{"kind":"fixture","nested":{"key":1,"key":2}}"#.as_slice(),
            br#"{"kind":"fixture","kind":"fixture"}"#.as_slice(),
            br#"{"kind":"legacy","command":"stop"}"#.as_slice(),
            br#"{"kind":"updater","command":"stop"}"#.as_slice(),
            br#"{"kind":"automation","command":"request"}"#.as_slice(),
        ] {
            assert_ne!(
                Fixture::status(&fixture.request(&fixture.headers(body.len()), body)),
                200
            );
        }
        assert_eq!(fixture.extension.mutations.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn http_enforces_extension_request_response_and_header_bounds() {
        let fixture = Fixture::new(REPLY);
        let mut maximum = REQUEST.to_vec();
        maximum.resize(255, b' ');
        assert_eq!(
            Fixture::status(&fixture.request(&fixture.headers(maximum.len()), &maximum)),
            200
        );
        maximum.push(b' ');
        assert_eq!(
            Fixture::status(&fixture.request(&fixture.headers(maximum.len()), &maximum)),
            413
        );
        let oversized_header = format!(
            "{}X-Large: {}\r\n",
            fixture.headers(REQUEST.len()),
            "x".repeat(MAX_HEADER_BYTES)
        );
        assert_ne!(
            Fixture::status(&fixture.request(&oversized_header, REQUEST)),
            200
        );
        assert_eq!(fixture.extension.mutations.load(Ordering::SeqCst), 1);
        let mut exact_response = b"{}".to_vec();
        exact_response.resize(127, b' ');
        exact_response.push(b'\n');
        let exact = Fixture::new(&exact_response);
        let response = exact.request(&exact.headers(REQUEST.len()), REQUEST);
        assert_eq!(Fixture::status(&response), 200);
        assert!(response.ends_with(&exact_response));
        exact_response.insert(2, b' ');
        for response in [
            exact_response,
            b"{\"ok\":true,\"ok\":false}\n".to_vec(),
            b"{}\n{}\n".to_vec(),
            b"{}".to_vec(),
        ] {
            let invalid = Fixture::new(&response);
            assert_ne!(
                Fixture::status(&invalid.request(&invalid.headers(REQUEST.len()), REQUEST)),
                200
            );
        }
    }

    #[test]
    fn shutdown_bounds_stalled_clients_retires_descriptor_and_rejects_old_tokens() {
        let mut fixture = Fixture::new(REPLY);
        let old_token = fixture.token();
        let old_host = fixture.host();
        let mut stalled = TcpStream::connect(&old_host).unwrap();
        stalled.write_all(b"POST /extension HTTP/1.1\r\n").unwrap();
        let descriptor_path = fixture.server.inner.paths.state_dir.join(DESCRIPTOR_FILE);
        let started = Instant::now();
        fixture.server.shutdown();
        assert!(started.elapsed() < CLIENT_TIMEOUT + Duration::from_secs(1));
        assert!(!descriptor_path.exists());
        assert!(TcpStream::connect(&old_host).is_err());
        let inner = &fixture.server.inner;
        {
            let mut state = lock_unpoisoned(&inner.shared);
            let mut context = state.update_context().unwrap().clone();
            context.instance_id = "daemon-http-2".into();
            state.set_update_context(context).unwrap();
        }
        let mut replacement = ControlServer::bind(
            inner.shared.clone(),
            inner.paths.clone(),
            inner.executable_path.clone(),
            inner.packs.clone(),
            inner.extensions.clone(),
            Vec::new(),
        )
        .unwrap();
        replacement.start().unwrap();
        fixture.server = replacement;
        assert_ne!(fixture.token(), old_token);
        assert_eq!(fixture.descriptor()["instance_id"], "daemon-http-2");
        let header = fixture
            .headers(REQUEST.len())
            .replace(&fixture.token(), &old_token);
        assert_eq!(Fixture::status(&fixture.request(&header, REQUEST)), 401);
        assert_eq!(fixture.extension.mutations.load(Ordering::SeqCst), 0);
        assert_eq!(
            Fixture::status(&fixture.request(&fixture.headers(REQUEST.len()), REQUEST)),
            200
        );
    }

    #[test]
    fn shutdown_does_not_remove_another_instances_descriptor() {
        let mut fixture = Fixture::new(REPLY);
        let path = fixture.server.inner.paths.state_dir.join(DESCRIPTOR_FILE);
        let mut replacement = fixture.descriptor();
        replacement["instance_id"] = Value::String("replacement-instance".into());
        replacement["token"] = Value::String("b".repeat(64));
        let bytes = serde_json::to_vec(&replacement).unwrap();
        desktop_pet_contracts::profile_store::atomic_write(&path, &bytes).unwrap();
        fixture.server.shutdown();
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }
    #[test]
    fn publication_failure_preserves_foreign_symlink_and_closes_listeners() {
        let mut fixture = Fixture::new(REPLY);
        fixture.server.shutdown();
        let inner = &fixture.server.inner;
        let descriptor_path = inner.paths.state_dir.join(DESCRIPTOR_FILE);
        let foreign = inner.paths.state_dir.join("foreign.json");
        fs::write(&foreign, b"foreign descriptor").unwrap();
        std::os::unix::fs::symlink(&foreign, &descriptor_path).unwrap();
        let mut replacement = ControlServer::bind(
            inner.shared.clone(),
            inner.paths.clone(),
            inner.executable_path.clone(),
            inner.packs.clone(),
            inner.extensions.clone(),
            Vec::new(),
        )
        .unwrap();
        let address = replacement
            .extension_http
            .as_ref()
            .unwrap()
            .descriptor
            .url
            .strip_prefix("http://")
            .unwrap()
            .strip_suffix("/extension")
            .unwrap()
            .to_owned();
        assert!(replacement.start().is_err());
        assert!(TcpStream::connect(address).is_err());
        assert!(!inner.paths.control_socket.exists());
        assert!(fs::symlink_metadata(&descriptor_path)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(fs::read(foreign).unwrap(), b"foreign descriptor");
    }

    #[test]
    fn partial_http_requests_expire_without_late_mutation() {
        let fixture = Fixture::new(REPLY);
        let mut stream = TcpStream::connect(fixture.host()).unwrap();
        stream
            .set_read_timeout(Some(CLIENT_TIMEOUT + Duration::from_secs(1)))
            .unwrap();
        let header = fixture.headers(REQUEST.len());
        stream.write_all(header.as_bytes()).unwrap();
        stream.write_all(b"\r\n{").unwrap();
        let started = Instant::now();
        let mut response = Vec::new();
        match stream.read_to_end(&mut response) {
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::ConnectionReset => {}
            Err(error) => panic!("stalled request did not close: {error}"),
        }
        assert!(started.elapsed() < CLIENT_TIMEOUT + Duration::from_secs(1));
        let _ = stream.write_all(&REQUEST[1..]);
        assert_eq!(fixture.extension.mutations.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn saturated_idle_http_preserves_private_control_capacity_and_bounded_shutdown() {
        let mut fixture = Fixture::new(REPLY);
        let host = fixture.host();
        let mut idle_http = Vec::new();
        for _ in 0..MAX_HTTP_CONNECTIONS {
            idle_http.push(TcpStream::connect(&host).unwrap());
        }
        // FIFO admission makes the next connection a public observation that
        // all HTTP slots are occupied, without inspecting server internals.
        let mut excess = TcpStream::connect(&host).unwrap();
        excess
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        let mut byte = [0u8; 1];
        match excess.read(&mut byte) {
            Ok(0) => {}
            Err(error) if error.kind() == io::ErrorKind::ConnectionReset => {}
            result => panic!("excess HTTP connection was not rejected: {result:?}"),
        }
        // Preserve the complete protected Unix budget, not merely one spare
        // slot: even under HTTP saturation its final slot must still dispatch.
        let path = &fixture.server.inner.paths.control_socket;
        let mut idle_unix = Vec::new();
        for _ in 0..MAX_CONNECTIONS - 1 {
            let stream = UnixStream::connect(path).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(1)))
                .unwrap();
            idle_unix.push(stream);
        }
        let mut control = UnixStream::connect(path).unwrap();
        control
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        control.write_all(REQUEST).unwrap();
        control.write_all(b"\n").unwrap();
        let mut response = Vec::new();
        control.read_to_end(&mut response).unwrap();
        assert_eq!(response, REPLY);
        assert_eq!(fixture.extension.mutations.load(Ordering::SeqCst), 1);
        let started = Instant::now();
        fixture.server.shutdown();
        assert!(started.elapsed() < CLIENT_TIMEOUT + Duration::from_secs(1));
        for mut stream in idle_http {
            stream
                .set_read_timeout(Some(Duration::from_secs(1)))
                .unwrap();
            let mut response = Vec::new();
            match stream.read_to_end(&mut response) {
                Ok(_) => {}
                Err(error) if error.kind() == io::ErrorKind::ConnectionReset => {}
                Err(error) => panic!("idle HTTP connection survived shutdown: {error}"),
            }
            assert!(!response.starts_with(b"HTTP/1.1 200 "));
        }
        for mut stream in idle_unix {
            assert_eq!(stream.read(&mut byte).unwrap(), 0);
        }
    }

    #[test]
    fn no_extensions_keeps_bare_core_private_control_without_http_discovery() {
        let mut fixture = Fixture::new(REPLY);
        fixture.server.shutdown();
        let inner = &fixture.server.inner;
        // A bare composition needs neither extension identity nor profile ports.
        let mut bare = ControlServer::bind(
            Arc::new(Mutex::new(AppState::new())),
            inner.paths.clone(),
            inner.executable_path.clone(),
            inner.packs.clone(),
            Vec::new(),
            Vec::new(),
        )
        .unwrap();
        bare.start().unwrap();
        assert!(!inner.paths.state_dir.join(DESCRIPTOR_FILE).exists());
        let mut stream = UnixStream::connect(&inner.paths.control_socket).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        stream
            .write_all(b"{\"version\":1,\"command\":\"ready\"}\n")
            .unwrap();
        let mut bytes = Vec::new();
        stream.read_to_end(&mut bytes).unwrap();
        let response: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(response["ok"], true);
        assert_eq!(response["control_ready"], true);
        bare.shutdown();
        assert!(!inner.paths.state_dir.join(DESCRIPTOR_FILE).exists());
    }
}
