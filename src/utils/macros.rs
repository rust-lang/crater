macro_rules! string_enum {
    (pub enum $name:ident { $($item:ident => $str:expr,)* }) => {
        #[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Hash, Copy, Clone)]
        pub enum $name {
            $($item,)*
        }

        impl ::std::str::FromStr for $name {
            type Err = ::errors::Error;

            fn from_str(s: &str) -> ::errors::Result<$name> {
                Ok(match s {
                    $($str => $name::$item,)*
                    s => bail!("invalid {}: {}", stringify!($name), s),
                })
            }
        }

        impl ::std::fmt::Display for $name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
                write!(f, "{}", self.to_str())
            }
        }

        impl $name {
            pub fn to_str(&self) -> &'static str {
                match *self {
                    $($name::$item => $str,)*
                }
            }

            pub fn possible_values() -> &'static [&'static str] {
                &[$($str,)*]
            }
        }
    }
}
