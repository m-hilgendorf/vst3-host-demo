use super::Module;
use crate::error::Error;
use std::path::Path;

impl Module {
    pub fn try_open(_path: impl AsRef<Path>) -> Result<Option<Self>, Error> {
        todo!()
    }
}
