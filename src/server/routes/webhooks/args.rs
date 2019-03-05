use crate::experiments::{Assignee, CapLints, CrateSelect, Mode};
use crate::toolchain::Toolchain;

#[derive(Debug, Fail)]
#[cfg_attr(test, derive(PartialEq, Eq))]
pub enum CommandParseError {
    #[fail(display = "missing command")]
    MissingCommand,
    #[fail(display = "invalid argument: {}", _0)]
    InvalidArgument(String),
    #[fail(display = "duplicate key: {}", _0)]
    DuplicateKey(String),
    #[fail(display = "unknown key: {}", _0)]
    UnknownKey(String),
}

macro_rules! generate_parser {
    (pub enum $enum:ident {
        $($command:expr => $variant:ident($var_struct:ident {
            $($flag:ident: $type:ty = $name:expr,)*
        }))*
        _ => $d_variant:ident($d_var_struct:ident {$($d_flag:ident: $d_type:ty = $d_name:expr,)*})
    }) => {
        use crate::prelude::*;
        use std::str::FromStr;
        use crate::utils::string::split_quoted;

        $(
            #[cfg_attr(test, derive(Debug, PartialEq))]
            pub struct $var_struct {
                $(pub $flag: $type,)*
            }
        )*

        #[cfg_attr(test, derive(Debug, PartialEq))]
        pub struct $d_var_struct {
            $(pub $d_flag: $d_type,)*
        }

        #[cfg_attr(test, derive(Debug, PartialEq))]
        pub enum $enum {
            $d_variant($d_var_struct),
            $($variant($var_struct),)*
        }

        #[allow(unused_variables, unused_mut)]
        impl FromStr for $enum {
            type Err = ::failure::Error;

            fn from_str(input: &str) -> Fallible<$enum> {
                let mut parts = split_quoted(input)?.into_iter().peekable();
                Ok(match parts.peek().map(|s| s.as_str()) {
                    $(
                        Some($command) => generate_parser!(@parser
                            parts.skip(1), $enum, $variant, $var_struct,
                            $($flag, $type, $name),*
                        ),
                    )*
                    Some(_) => generate_parser!(@parser
                        parts, $enum, $d_variant, $d_var_struct,
                        $($d_flag, $d_type, $d_name),*
                    ),
                    _ => return Err(CommandParseError::MissingCommand.into()),
                })
            }
        }
    };

    (@parser
        $parts:expr, $enum:ident, $variant:ident, $var_struct:ident,
        $($flag:ident, $type:ty, $name:expr),*
    ) => {{
        let mut args = $var_struct {
            $($flag: None,)*
        };

        for part in $parts {
            if part.trim() == "" {
                continue;
            }

            let mut segments = part.splitn(2, '=');
            let key = segments.next().ok_or_else(|| CommandParseError::InvalidArgument(part.to_string()))?;
            let value = segments.next().ok_or_else(|| CommandParseError::InvalidArgument(part.to_string()))?;

            if false {}
            $(else if key == $name {
                if args.$flag.is_none() {
                    args.$flag = Some(value.parse()?)
                } else {
                    return Err(CommandParseError::DuplicateKey(key.to_string()).into());
                }
            })*
            else {
                return Err(CommandParseError::UnknownKey(key.to_string()).into());
            }
        }

        $enum::$variant(args)
    }};
}

generate_parser!(pub enum Command {
    "run" => Run(RunArgs {
        name: Option<String> = "name",
        start: Option<Toolchain> = "start",
        end: Option<Toolchain> = "end",
        mode: Option<Mode> = "mode",
        crates: Option<CrateSelect> = "crates",
        cap_lints: Option<CapLints> = "cap-lints",
        priority: Option<i32> = "p",
        ignore_blacklist: Option<bool> = "ignore-blacklist",
        assign: Option<Assignee> = "assign",
    })

    "abort" => Abort(AbortArgs {
        name: Option<String> = "name",
    })

    "ping" => Ping(PingArgs {})

    "retry-report" => RetryReport(RetryReportArgs {
        name: Option<String> = "name",
    })

    "retry" => Retry(RetryArgs {
        name: Option<String> = "name",
    })

    "reload-acl" => ReloadACL(ReloadACLArgs {})

    _ => Edit(EditArgs {
        name: Option<String> = "name",
        start: Option<Toolchain> = "start",
        end: Option<Toolchain> = "end",
        mode: Option<Mode> = "mode",
        crates: Option<CrateSelect> = "crates",
        cap_lints: Option<CapLints> = "cap-lints",
        priority: Option<i32> = "p",
        ignore_blacklist: Option<bool> = "ignore-blacklist",
        assign: Option<Assignee> = "assign",
    })
});

#[cfg(test)]
mod tests {
    use super::CommandParseError;

    // Use a simpler parser for tests
    generate_parser!(pub enum TestCommand {
        "foo" => Foo(FooArgs {
            arg1: Option<i32> = "arg1",
            arg2: Option<String> = "arg2",
        })

        "bar" => Bar(BarArgs {
            arg3: Option<String> = "arg3",
        })

        _ => Baz(BazArgs {
            arg4: Option<i32> = "arg4",
        })
    });

    #[test]
    fn test_command_parsing() {
        macro_rules! test {
            ($cmd:expr, $expected:expr) => {
                assert_eq!($cmd.parse::<TestCommand>().unwrap(), $expected);
            };
            (fail $cmd:expr, $error:expr) => {
                assert_eq!(
                    $cmd.parse::<TestCommand>().unwrap_err().downcast_ref(),
                    Some(&$error)
                );
            };
        }

        // Test if the right command is recognized
        test!(
            "foo",
            TestCommand::Foo(FooArgs {
                arg1: None,
                arg2: None,
            })
        );
        test!("bar", TestCommand::Bar(BarArgs { arg3: None }));
        test!("", TestCommand::Baz(BazArgs { arg4: None }));

        // Test if args are parsed correctly
        test!(
            "foo arg1=98",
            TestCommand::Foo(FooArgs {
                arg1: Some(98),
                arg2: None,
            })
        );
        test!(
            "foo arg2=bar arg1=98",
            TestCommand::Foo(FooArgs {
                arg1: Some(98),
                arg2: Some("bar".into()),
            })
        );
        test!(
            "bar  arg3=foo=bar",
            TestCommand::Bar(BarArgs {
                arg3: Some("foo=bar".into()),
            })
        );
        test!(
            "bar arg3=\"foo \\\" bar\"",
            TestCommand::Bar(BarArgs {
                arg3: Some("foo \" bar".into()),
            })
        );
        test!("arg4=42", TestCommand::Baz(BazArgs { arg4: Some(42) }));

        // Test if invalid args are rejected
        test!(fail "foo arg1=98 arg1=42", CommandParseError::DuplicateKey("arg1".into()));
        test!(fail "bar arg1=98", CommandParseError::UnknownKey("arg1".into()));
        test!(fail "foo arg4=42", CommandParseError::UnknownKey("arg4".into()));
        test!(fail "foo bar", CommandParseError::InvalidArgument("bar".into()));
    }
}
