use errors::*;
use serde::{Deserialize, Serialize};

pub fn checkpoint<Load, Resolve, Instruction, Resolution>
    (name: &str,
     load: Load,
     resolve: Resolve)
     -> Result<Vec<(Instruction, Resolution)>>
    where Load: FnMut() -> Result<Vec<Instruction>>,
          Resolve: FnMut(&Instruction) -> Result<Resolution>,
          Instruction: Serialize + Deserialize + Eq,
          Resolution: Serialize + Deserialize
{
    panic!()
}
