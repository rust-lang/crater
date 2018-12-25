use crate::prelude::*;

#[derive(Debug, Fail)]
#[cfg_attr(test, derive(PartialEq, Eq))]
pub(crate) enum HexError {
    #[fail(display = "invalid char in hex: {}", _0)]
    InvalidChar(char),
    #[fail(display = "invalid hex length")]
    InvalidLength,
}

pub(crate) fn from_hex(input: &str) -> Result<Vec<u8>, HexError> {
    let mut result = Vec::with_capacity(input.len() / 2);

    let mut pending: u8 = 0;
    let mut buffer: u8 = 0;
    let mut current: u8;
    for (i, byte) in input.bytes().enumerate() {
        pending += 1;

        current = match byte {
            b'0'...b'9' => byte - b'0',
            b'a'...b'f' => byte - b'a' + 10,
            b'A'...b'F' => byte - b'A' + 10,
            _ => {
                return Err(HexError::InvalidChar(input[i..].chars().next().unwrap()));
            }
        };

        if pending == 1 {
            buffer = current;
        } else {
            result.push(buffer * 16 + current);
            pending = 0;
        }
    }

    if pending != 0 {
        Err(HexError::InvalidLength)
    } else {
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::{from_hex, HexError};

    #[test]
    fn test_from_hex() {
        assert_eq!(
            from_hex("00010210ffFfFF").unwrap(),
            vec![0x00, 0x01, 0x02, 0x10, 0xFF, 0xFF, 0xFF]
        );

        // Invalid char
        assert_eq!(from_hex("!").unwrap_err(), HexError::InvalidChar('!'));
        assert_eq!(from_hex("g").unwrap_err(), HexError::InvalidChar('g'));

        // Invalid length
        assert_eq!(from_hex("000").unwrap_err(), HexError::InvalidLength);
    }
}
