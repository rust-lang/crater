use serde::ser::{Serialize, SerializeSeq, Serializer};

pub fn to_vec<S, T>(data: T, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
    T: IntoIterator,
    T::Item: Serialize,
    T::IntoIter: std::iter::ExactSizeIterator,
{
    let data = IntoIterator::into_iter(data);
    let mut seq = serializer.serialize_seq(Some(data.len()))?;
    for element in data {
        seq.serialize_element(&element)?;
    }
    seq.end()
}
