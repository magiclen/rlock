use std::{
    fmt::{self, Debug, Formatter},
    ops::{Deref, DerefMut},
    sync::Mutex,
    time::Duration,
};

use redis::{Client, Connection, ConnectionLike, RedisResult};

const DEFAULT_MAX_IDLE_CONNECTIONS: usize = 8;
const DEFAULT_CONNECTION_TIMEOUT: Option<Duration> = None;

/// Options used when building the connection pool of a sync `RLock` manager.
///
/// # Examples
///
/// ```rust
/// use std::time::Duration;
///
/// use rlock::sync_lock::PoolOptions;
///
/// let options = PoolOptions::new()
///     .max_idle_connections(4)
///     .connection_timeout(Some(Duration::from_secs(3)));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PoolOptions {
    pub(crate) max_idle_connections: usize,
    pub(crate) connection_timeout:   Option<Duration>,
}

impl Default for PoolOptions {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl PoolOptions {
    /// Creates the default pool options.
    #[inline]
    pub const fn new() -> Self {
        Self {
            max_idle_connections: DEFAULT_MAX_IDLE_CONNECTIONS,
            connection_timeout:   DEFAULT_CONNECTION_TIMEOUT,
        }
    }

    /// Maximum number of idle connections kept in the pool for reuse.
    ///
    /// A returned connection which exceeds this limit is dropped instead of being kept.
    #[inline]
    pub const fn max_idle_connections(mut self, max_idle_connections: usize) -> Self {
        self.max_idle_connections = max_idle_connections;

        self
    }

    /// Maximum duration to wait when establishing a new connection.
    ///
    /// If `None`, the default connection behavior of the Redis client is used.
    #[inline]
    pub const fn connection_timeout(mut self, duration: Option<Duration>) -> Self {
        self.connection_timeout = duration;

        self
    }
}

/// A simple connection pool which keeps a limited number of idle connections for reuse.
///
/// # Behavior
///
/// * When no idle connection is available, a new connection is created, so checking out never blocks.
/// * A connection which has hit an IO error is not open anymore, so it is dropped instead of being returned to the pool, and a following checkout creates a fresh connection. This approximates the transparent reconnection of the async connection manager.
/// * The pool does not ping a connection on checkout and does not recycle idle connections by age. A stale connection which has been closed by the server costs one failed call before it is discarded.
pub(crate) struct ConnectionPool {
    client:  Client,
    idle:    Mutex<Vec<Connection>>,
    options: PoolOptions,
}

impl Debug for ConnectionPool {
    #[inline]
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConnectionPool").field("options", &self.options).finish_non_exhaustive()
    }
}

impl ConnectionPool {
    #[inline]
    pub(crate) const fn new(client: Client, options: PoolOptions) -> Self {
        Self {
            client,
            idle: Mutex::new(Vec::new()),
            options,
        }
    }

    #[inline]
    fn connect(&self) -> RedisResult<Connection> {
        match self.options.connection_timeout {
            Some(timeout) => self.client.get_connection_with_timeout(timeout),
            None => self.client.get_connection(),
        }
    }

    /// Checks out a connection from the pool, or creates a new one if no idle connection is available.
    pub(crate) fn checkout(&self) -> RedisResult<PooledConnection<'_>> {
        // pop the connection first so the mutex is not held while doing IO
        let conn = self.idle.lock().unwrap().pop();

        let conn = match conn {
            Some(conn) => conn,
            None => self.connect()?,
        };

        Ok(PooledConnection {
            pool: self, conn: Some(conn)
        })
    }

    /// Returns a connection to the pool, dropping it if it is broken or if the pool is full.
    fn checkin(&self, conn: Connection) {
        if !conn.is_open() {
            return;
        }

        let mut idle = self.idle.lock().unwrap();

        if idle.len() < self.options.max_idle_connections {
            idle.push(conn);
        }
    }
}

/// A checked-out connection which returns itself to the pool when it is dropped.
pub(crate) struct PooledConnection<'a> {
    pool: &'a ConnectionPool,
    conn: Option<Connection>,
}

impl Deref for PooledConnection<'_> {
    type Target = Connection;

    #[inline]
    fn deref(&self) -> &Connection {
        self.conn.as_ref().unwrap()
    }
}

impl DerefMut for PooledConnection<'_> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Connection {
        self.conn.as_mut().unwrap()
    }
}

impl Drop for PooledConnection<'_> {
    #[inline]
    fn drop(&mut self) {
        if let Some(conn) = self.conn.take() {
            self.pool.checkin(conn);
        }
    }
}
