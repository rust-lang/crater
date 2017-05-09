use errors::*;
use serde::{Deserialize, Serialize};
use serde_json;
use std::marker::PhantomData;
use std::path::PathBuf;

pub struct Blobject<T>(PathBuf, PhantomData<T>) where T: Serialize + Deserialize;

impl<T> Blobject<T>
    where T: Serialize + Deserialize
{
    fn write(&self, v: &T) -> Result<()> {
        panic!();
    }

    fn read(&self) -> Result<Option<T>> {
        panic!();
    }
}
