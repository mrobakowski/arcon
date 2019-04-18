use crate::error::*;

pub trait StateBackend: Send + Sync {
    fn create(name: &str) -> Self
    where
        Self: Sized;
    fn put(&mut self, key: &[u8], value: &[u8]) -> Result<()>;
    fn get(&self, key: &[u8]) -> Result<Vec<u8>>;
    fn checkpoint(&self, id: String) -> Result<()>;
}
