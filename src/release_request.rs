use std::sync::Arc;

#[derive(Debug, Clone)]
pub(crate) enum ReleaseRequestKey {
    Single(Arc<String>),
    Multiple(Arc<Vec<String>>),
    Read(Arc<String>),
    Write(Arc<String>),
    MultipleRead(Arc<Vec<String>>),
    MultipleWrite(Arc<Vec<String>>),
}

#[derive(Debug, Clone)]
pub(crate) struct ReleaseRequest {
    pub(crate) key:  ReleaseRequestKey,
    pub(crate) uuid: Arc<String>,
}
