macro_rules! string_enum {
    ($vis:vis enum $name:ident { $($item:ident => $str:expr,)* }) => {
        #[derive(Debug, PartialEq, Eq, Hash, Copy, Clone, Serialize, Deserialize)]
        #[serde(try_from = "String", into = "String")]
        $vis enum $name {
            $($item,)*
        }

        impl ::std::str::FromStr for $name {
            type Err = ::anyhow::Error;

            fn from_str(s: &str) -> ::anyhow::Result<$name> {
                match s {
                    $($str => Ok($name::$item),)*
                    s => bail!("invalid {}: {}", stringify!($name), s),
                }
            }
        }

        impl ::std::fmt::Display for $name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
                write!(f, "{}", self.to_str())
            }
        }

        impl $name {
            #[allow(dead_code)]
            $vis fn to_str(&self) -> &'static str {
                match *self {
                    $($name::$item => $str,)*
                }
            }

            #[allow(dead_code)]
            $vis fn possible_values() -> &'static [&'static str] {
                &[$($str,)*]
            }
        }

        from_into_string!($name);
    }
}

macro_rules! from_into_string {
    ($for:ident) => {
        impl std::convert::TryFrom<String> for $for {
            type Error = <$for as std::str::FromStr>::Err;
            fn try_from(s: String) -> Result<Self, <$for as std::str::FromStr>::Err> {
                s.parse()
            }
        }

        impl From<$for> for String {
            fn from(s: $for) -> String {
                s.to_string()
            }
        }
    };
}

macro_rules! btreeset {
    ($($x:expr),+ $(,)?) => (
        vec![$($x),+].into_iter().collect::<BTreeSet<_>>()
    );
}
