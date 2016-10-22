use serde::{Deserialize, Serialize};
use errors::*;

pub fn checkpoint<Load, Resolve, Instruction, Resolution>(
    name: &str,
    load: Load,
    resolve: Resolve) -> Result<Vec<(Instruction, Resolution)>>
    where
    Load: FnMut() -> Result<Vec<Instruction>>,
    Resolve: FnMut(&Instruction) -> Result<Resolution>,
    Instruction: Serialize + Deserialize + Eq,
    Resolution: Serialize + Deserialize
{
    panic!()
}
