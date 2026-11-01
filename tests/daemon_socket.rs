#![cfg(unix)]

use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::{symlink, DirBuilderExt, MetadataExt};
use std::os::unix::net::{UnixDatagram, UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);
const DEADLINE: Duration = Duration::from_secs(5);

struct Directory(PathBuf);

impl Directory {
    fn new() -> Self {
        loop {
            let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let path =
                std::env::temp_dir().join(format!("ayeosd-test-{}-{sequence}", std::process::id()));
            match fs::DirBuilder::new().mode(0o700).create(&path) {
                Ok(()) => return Self(path),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("create test directory: {error}"),
            }
        }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct Daemon(Child);

impl Daemon {
    fn start(path: &Path, tcp: Option<u16>) -> Self {
        let mut command = Command::new(env!("CARGO_BIN_EXE_ayeosd"));
        command
            .env("AYEOSD_SOCK", path)
            .env_remove("AYEOSD_TCP")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        if let Some(port) = tcp {
            command.env("AYEOSD_TCP", port.to_string());
        }
        Self(command.spawn().unwrap())
    }

    fn wait_for_socket(&mut self, path: &Path) {
        let deadline = Instant::now() + DEADLINE;
        loop {
            assert!(self.0.try_wait().unwrap().is_none(), "daemon exited early");
            if let Ok(stream) = UnixStream::connect(path) {
                stream.set_read_timeout(Some(DEADLINE)).unwrap();
                stream.set_write_timeout(Some(DEADLINE)).unwrap();
                assert_eq!(request(stream, "ping"), "pong");
                return;
            }
            assert!(Instant::now() < deadline, "daemon did not create socket");
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn expect_refusal(&mut self) -> String {
        let deadline = Instant::now() + DEADLINE;
        loop {
            if let Some(status) = self.0.try_wait().unwrap() {
                assert_eq!(status.code(), Some(1));
                let mut error = String::new();
                self.0
                    .stderr
                    .take()
                    .unwrap()
                    .read_to_string(&mut error)
                    .unwrap();
                return error;
            }
            assert!(Instant::now() < deadline, "daemon should refuse this path");
            thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn request(mut stream: impl Read + Write, command: &str) -> String {
    writeln!(stream, "{command}").unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    response
}

fn identity(path: &Path) -> (u64, u64) {
    let metadata = fs::symlink_metadata(path).unwrap();
    (metadata.dev(), metadata.ino())
}

#[test]
fn preserves_files_symlinks_directories_and_other_socket_types() {
    let directory = Directory::new();
    let file = directory.path("file");
    fs::write(&file, b"owner data\n").unwrap();
    fs::create_dir(directory.path("directory")).unwrap();
    symlink(&file, directory.path("file-link")).unwrap();
    symlink(directory.path("absent"), directory.path("dangling-link")).unwrap();
    let target = directory.path("socket-target");
    let _listener = UnixListener::bind(&target).unwrap();
    symlink(&target, directory.path("socket-link")).unwrap();
    let _datagram = UnixDatagram::bind(directory.path("datagram")).unwrap();
    let target_identity = identity(&target);

    for name in [
        "file",
        "directory",
        "file-link",
        "dangling-link",
        "socket-link",
        "datagram",
    ] {
        let path = directory.path(name);
        let before = identity(&path);
        let error = Daemon::start(&path, None).expect_refusal();
        assert!(error.contains("ayeosd:"), "{error}");
        assert_eq!(identity(&path), before, "replaced {name}");
        assert_eq!(fs::read(&file).unwrap(), b"owner data\n");
        assert_eq!(identity(&target), target_identity);
    }
    assert_eq!(fs::read_link(directory.path("file-link")).unwrap(), file);
    assert_eq!(
        fs::read_link(directory.path("socket-link")).unwrap(),
        target
    );
    assert_eq!(
        fs::read_link(directory.path("dangling-link")).unwrap(),
        directory.path("absent")
    );
}

#[test]
fn recovers_stale_socket_and_refuses_live_instance() {
    let directory = Directory::new();
    let path = directory.path("daemon.sock");
    drop(UnixListener::bind(&path).unwrap());
    let mut first = Daemon::start(&path, None);
    first.wait_for_socket(&path);
    let before = identity(&path);
    let error = Daemon::start(&path, None).expect_refusal();
    assert!(error.contains("already listening"), "{error}");
    assert_eq!(identity(&path), before);
    first.wait_for_socket(&path);
}

#[test]
fn socket_override_serves_protocol_without_default_tcp() {
    let directory = Directory::new();
    let path = directory.path("custom.sock");
    let mut daemon = Daemon::start(&path, None);
    daemon.wait_for_socket(&path);
    let stream = UnixStream::connect(&path).unwrap();
    stream.set_read_timeout(Some(DEADLINE)).unwrap();
    stream.set_write_timeout(Some(DEADLINE)).unwrap();
    assert_eq!(request(stream, "list"), r#"{"capsules":["genesis"]}"#);

    #[cfg(target_os = "linux")]
    {
        let process = PathBuf::from(format!("/proc/{}", daemon.0.id()));
        let sockets: Vec<String> = fs::read_dir(process.join("fd"))
            .unwrap()
            .filter_map(|entry| fs::read_link(entry.ok()?.path()).ok())
            .map(|path| path.to_string_lossy().into_owned())
            .collect();
        for table in ["tcp", "tcp6"] {
            let contents = fs::read_to_string(process.join("net").join(table)).unwrap();
            for line in contents.lines().skip(1) {
                let inode = line.split_whitespace().nth(9).unwrap();
                assert!(
                    !sockets.contains(&format!("socket:[{inode}]")),
                    "unexpected TCP socket"
                );
            }
        }
    }
}

#[test]
fn tcp_listener_requires_explicit_opt_in() {
    let directory = Directory::new();
    let path = directory.path("daemon.sock");
    let reservation = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = reservation.local_addr().unwrap().port();
    drop(reservation);
    let mut daemon = Daemon::start(&path, Some(port));
    daemon.wait_for_socket(&path);
    let stream = TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), DEADLINE).unwrap();
    stream.set_read_timeout(Some(DEADLINE)).unwrap();
    stream.set_write_timeout(Some(DEADLINE)).unwrap();
    assert_eq!(request(stream, "ping"), "pong");
}

#[test]
fn missing_parent_is_reported_without_creating_it() {
    let directory = Directory::new();
    let parent = directory.path("absent");
    let error = Daemon::start(&parent.join("daemon.sock"), None).expect_refusal();
    assert!(error.contains("failed to bind unix socket"), "{error}");
    assert!(!parent.exists());
}
