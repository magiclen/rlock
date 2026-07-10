use std::sync::Arc;

#[derive(Debug, Clone)]
pub(crate) enum ReleaseRequestKey {
    Single(Arc<String>),
    Multiple(Arc<Vec<String>>),
    // the read-write lock variants are gated because a sync-only build never constructs them and the dead-code lint would fail the build
    // TODO: change the gates to plain variants once the synchronous read-write lock is implemented
    #[cfg(feature = "async")]
    Read(Arc<String>),
    #[cfg(feature = "async")]
    Write(Arc<String>),
    #[cfg(feature = "async")]
    MultipleRead(Arc<Vec<String>>),
    #[cfg(feature = "async")]
    MultipleWrite(Arc<Vec<String>>),
}

#[derive(Debug, Clone)]
pub(crate) struct ReleaseRequest {
    pub(crate) key:  ReleaseRequestKey,
    pub(crate) uuid: Arc<String>,
}
