use errors::*;

pub(crate) fn from_hex(input: &str) -> Result<Vec<u8>> {
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
                bail!("invalid char {} in hex", input[i..].chars().next().unwrap());
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
        bail!("invalid hex length");
    } else {
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::from_hex;

    #[test]
    fn test_from_hex() {
        assert_eq!(
            from_hex("00010210ffFfFF").unwrap(),
            vec![0x00, 0x01, 0x02, 0x10, 0xFF, 0xFF, 0xFF]
        );

        // Invalid char
        assert!(from_hex("!").is_err());
        assert!(from_hex("g").is_err());

        // Invalid length
        assert!(from_hex("000").is_err());
    }
}
