use crate::prelude::*;
use std::fmt;
use std::str::FromStr;

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub enum Size {
    Bytes(usize),
    Kilobytes(usize),
    Megabytes(usize),
    Gigabytes(usize),
    Terabytes(usize),
}

from_into_string!(Size);

impl Size {
    pub(crate) fn to_bytes(&self) -> usize {
        match self {
            Size::Bytes(b) => *b,
            Size::Kilobytes(kb) => kb * 1024,
            Size::Megabytes(mb) => mb * 1024 * 1024,
            Size::Gigabytes(gb) => gb * 1024 * 1024 * 1024,
            Size::Terabytes(tb) => tb * 1024 * 1024 * 1024 * 1024,
        }
    }
}

impl fmt::Display for Size {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Size::Bytes(count) => write!(f, "{}", count),
            Size::Kilobytes(count) => write!(f, "{}K", count),
            Size::Megabytes(count) => write!(f, "{}M", count),
            Size::Gigabytes(count) => write!(f, "{}G", count),
            Size::Terabytes(count) => write!(f, "{}T", count),
        }
    }
}

impl FromStr for Size {
    type Err = failure::Error;

    fn from_str(mut input: &str) -> Fallible<Size> {
        let mut last = input.chars().last().ok_or_else(|| err_msg("empty size"))?;

        // Eat a trailing 'b'
        if last == 'b' || last == 'B' {
            input = &input[..input.len() - 1];
            last = input.chars().last().ok_or_else(|| err_msg("empty size"))?;
        }

        if last == 'K' || last == 'k' {
            Ok(Size::Kilobytes(input[..input.len() - 1].parse()?))
        } else if last == 'M' || last == 'm' {
            Ok(Size::Megabytes(input[..input.len() - 1].parse()?))
        } else if last == 'G' || last == 'g' {
            Ok(Size::Gigabytes(input[..input.len() - 1].parse()?))
        } else if last == 'T' || last == 't' {
            Ok(Size::Terabytes(input[..input.len() - 1].parse()?))
        } else {
            Ok(Size::Bytes(input.parse()?))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Size;

    #[test]
    fn test_size() {
        assert_eq!("1234".parse::<Size>().unwrap(), Size::Bytes(1234));
        assert_eq!("1234B".parse::<Size>().unwrap(), Size::Bytes(1234));
        assert_eq!("1234b".parse::<Size>().unwrap(), Size::Bytes(1234));
        assert_eq!(Size::Bytes(1234).to_string(), "1234");
        assert_eq!(Size::Bytes(42).to_bytes(), 42);

        assert_eq!("1234K".parse::<Size>().unwrap(), Size::Kilobytes(1234));
        assert_eq!("1234k".parse::<Size>().unwrap(), Size::Kilobytes(1234));
        assert_eq!("1234KB".parse::<Size>().unwrap(), Size::Kilobytes(1234));
        assert_eq!("1234kb".parse::<Size>().unwrap(), Size::Kilobytes(1234));
        assert_eq!(Size::Kilobytes(1234).to_string(), "1234K");
        assert_eq!(Size::Kilobytes(42).to_bytes(), 42 * 1024);

        assert_eq!("1234M".parse::<Size>().unwrap(), Size::Megabytes(1234));
        assert_eq!("1234m".parse::<Size>().unwrap(), Size::Megabytes(1234));
        assert_eq!("1234MB".parse::<Size>().unwrap(), Size::Megabytes(1234));
        assert_eq!("1234mb".parse::<Size>().unwrap(), Size::Megabytes(1234));
        assert_eq!(Size::Megabytes(1234).to_string(), "1234M");
        assert_eq!(Size::Megabytes(42).to_bytes(), 42 * 1024 * 1024);

        assert_eq!("1234G".parse::<Size>().unwrap(), Size::Gigabytes(1234));
        assert_eq!("1234g".parse::<Size>().unwrap(), Size::Gigabytes(1234));
        assert_eq!("1234GB".parse::<Size>().unwrap(), Size::Gigabytes(1234));
        assert_eq!("1234Gb".parse::<Size>().unwrap(), Size::Gigabytes(1234));
        assert_eq!(Size::Gigabytes(1234).to_string(), "1234G");
        assert_eq!(Size::Gigabytes(42).to_bytes(), 42 * 1024 * 1024 * 1024);

        assert_eq!("1234T".parse::<Size>().unwrap(), Size::Terabytes(1234));
        assert_eq!("1234t".parse::<Size>().unwrap(), Size::Terabytes(1234));
        assert_eq!("1234TB".parse::<Size>().unwrap(), Size::Terabytes(1234));
        assert_eq!("1234Tb".parse::<Size>().unwrap(), Size::Terabytes(1234));
        assert_eq!(Size::Terabytes(1234).to_string(), "1234T");
        assert_eq!(
            Size::Terabytes(42).to_bytes(),
            42 * 1024 * 1024 * 1024 * 1024
        );
    }
}
