use prelude::*;

#[derive(Debug, Fail)]
pub enum SplitQuotedError {
    #[fail(display = "unbalanced quotes")]
    UnbalancedQuotes,
}

pub(crate) fn split_quoted(input: &str) -> Result<Vec<String>, SplitQuotedError> {
    let mut segments = Vec::new();
    let mut buffer = String::new();

    let mut is_quoted = false;
    let mut is_escaped = false;
    for chr in input.chars() {
        match chr {
            // Always add escaped chars
            _ if is_escaped => {
                buffer.push(chr);
                is_escaped = false;
            }
            // When a \ is encountered, push the next char
            '\\' => is_escaped = true,
            // When a " is encountered, toggle quoting
            '"' => is_quoted = !is_quoted,
            // Split with spaces only if we're not inside a quote
            ' ' | '\t' if !is_quoted => {
                segments.push(buffer);
                buffer = String::new();
            }
            // Otherwise push the char
            _ => buffer.push(chr),
        }
    }

    if is_quoted {
        Err(SplitQuotedError::UnbalancedQuotes)
    } else {
        segments.push(buffer);
        Ok(segments)
    }
}

#[cfg(test)]
mod tests {
    use super::split_quoted;

    #[test]
    fn test_split_quoted() {
        macro_rules! test_split_quoted {
            ($($input:expr => [$($segment:expr),*],)*) => {
                $(
                    assert_eq!(split_quoted($input).unwrap(), vec![$($segment.to_string()),*]);
                )*
            }
        }

        // Valid syntaxes
        test_split_quoted! {
            "" => [""],
            "     " => ["", "", "", "", "", ""],
            "a b  c de " => ["a", "b", "", "c", "de", ""],
            "a \\\" b" => ["a", "\"", "b"],
            "a\\ b c" => ["a b", "c"],
            "a \"b c \\\" d\" e" => ["a", "b c \" d", "e"],
            "a b=\"c d e\" f" => ["a", "b=c d e", "f"],
        };

        // Unbalanced quotes
        assert!(split_quoted("a b \" c").is_err());
    }
}
