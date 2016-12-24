use errors::*;
use std::path::PathBuf;
use serde::{Serialize, Deserialize};
use serde_json;
use std::marker::PhantomData;

pub struct Blobject<T>(PathBuf, PhantomData<T>)
    where T: Serialize + Deserialize;

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
