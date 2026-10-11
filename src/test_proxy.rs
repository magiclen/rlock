use std::{
    io::{self, BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

#[derive(Default)]
struct GateData {
    command: Option<(&'static str, String)>,
    blocked: bool,
    resumed: bool,
}

#[derive(Default)]
struct Gate {
    data:    Mutex<GateData>,
    changed: Condvar,
}

pub(crate) struct TestProxy {
    pub(crate) url: String,
    gate:           Arc<Gate>,
    stopped:        Arc<AtomicBool>,
    listener:       Option<thread::JoinHandle<()>>,
}

enum Target {
    Tcp(TcpStream),
    #[cfg(unix)]
    Unix(std::os::unix::net::UnixStream),
}

impl Read for Target {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        match self {
            Self::Tcp(stream) => stream.read(bytes),
            #[cfg(unix)]
            Self::Unix(stream) => stream.read(bytes),
        }
    }
}

impl Write for Target {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        match self {
            Self::Tcp(stream) => stream.write(bytes),
            #[cfg(unix)]
            Self::Unix(stream) => stream.write(bytes),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Self::Tcp(stream) => stream.flush(),
            #[cfg(unix)]
            Self::Unix(stream) => stream.flush(),
        }
    }
}

fn frame(reader: &mut impl BufRead) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    if reader.read_until(b'\n', &mut bytes)? == 0 {
        return Err(io::ErrorKind::UnexpectedEof.into());
    }
    match bytes[0] {
        b'$' | b'*' => {
            let length: i64 =
                std::str::from_utf8(&bytes[1..bytes.len() - 2]).unwrap().parse().unwrap();
            if length >= 0 {
                if bytes[0] == b'$' {
                    let start = bytes.len();
                    bytes.resize(start + length as usize + 2, 0);
                    reader.read_exact(&mut bytes[start..])?;
                } else {
                    for _ in 0..length {
                        bytes.extend(frame(reader)?);
                    }
                }
            }
        },
        _ => {},
    }
    Ok(bytes)
}

fn forward(client: TcpStream, target: &redis::ConnectionAddr, gate: &Gate) -> io::Result<()> {
    let target = match target {
        redis::ConnectionAddr::Tcp(host, port) => {
            Target::Tcp(TcpStream::connect((host.as_str(), *port))?)
        },
        #[cfg(unix)]
        redis::ConnectionAddr::Unix(path) => {
            Target::Unix(std::os::unix::net::UnixStream::connect(path)?)
        },
        _ => return Err(io::ErrorKind::Unsupported.into()),
    };
    client.set_nodelay(true)?;
    let mut client = BufReader::new(client);
    let mut target = BufReader::new(target);
    loop {
        let request = frame(&mut client)?;
        target.get_mut().write_all(&request)?;
        let response = frame(&mut target)?;
        let command = request.split(|byte| *byte == b'\n').nth(2).unwrap_or_default();
        let mut data = gate.data.lock().unwrap();
        let matches = data.command.as_ref().is_some_and(|(expected, key)| {
            let command = command.strip_suffix(b"\r").unwrap_or(command);
            (command == expected.as_bytes() || (*expected == "EVALSHA" && command == b"EVAL"))
                && request.windows(key.len()).any(|part| part == key.as_bytes())
                && response[0] != b'-'
        });
        if matches {
            // Hold only the reply after Redis has executed the command.
            data.command.take();
            data.blocked = true;
            gate.changed.notify_all();
            while !data.resumed {
                data = gate.changed.wait(data).unwrap();
            }
        }
        drop(data);
        client.get_mut().write_all(&response)?;
    }
}

impl TestProxy {
    pub(crate) fn new() -> Self {
        let redis_url =
            std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379/0".into());
        let target = redis::Client::open(redis_url).unwrap().get_connection_info().addr().clone();
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("redis://127.0.0.1:{}/0", listener.local_addr().unwrap().port());
        let gate = Arc::new(Gate::default());
        let stopped = Arc::new(AtomicBool::new(false));
        let worker_gate = gate.clone();
        let worker_stopped = stopped.clone();
        let listener = thread::spawn(move || {
            while !worker_stopped.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((client, _)) => {
                        let target = target.clone();
                        let gate = worker_gate.clone();
                        thread::spawn(move || {
                            let _ = forward(client, &target, &gate);
                        });
                    },
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(1))
                    },
                    Err(error) => panic!("cannot accept a test connection: {error}"),
                }
            }
        });
        Self {
            url,
            gate,
            stopped,
            listener: Some(listener),
        }
    }

    pub(crate) fn arm(&self, command: &'static str, key: &str) {
        *self.gate.data.lock().unwrap() =
            GateData {
                command: Some((command, key.to_owned())), blocked: false, resumed: false
            };
    }

    pub(crate) fn resume(&self) {
        self.gate.data.lock().unwrap().resumed = true;
        self.gate.changed.notify_all();
    }

    pub(crate) fn is_blocked(&self) -> bool {
        self.gate.data.lock().unwrap().blocked
    }

    #[cfg(feature = "async")]
    pub(crate) async fn wait_blocked(&self) {
        let start = Instant::now();
        while !self.is_blocked() {
            assert!(start.elapsed() < Duration::from_secs(3), "the Redis reply was not held");
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    }
}

impl Drop for TestProxy {
    fn drop(&mut self) {
        self.resume();
        self.stopped.store(true, Ordering::Release);
        self.listener.take().unwrap().join().unwrap();
    }
}

pub(crate) fn assert_key_released(key: &str) {
    let url = std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379/0".into());
    let mut connection = redis::Client::open(url).unwrap().get_connection().unwrap();
    let start = Instant::now();
    loop {
        if redis::cmd("EXISTS").arg(key).query::<usize>(&mut connection).unwrap() == 0 {
            return;
        }
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "the lock was not released before its TTL"
        );
        thread::sleep(Duration::from_millis(1));
    }
}
