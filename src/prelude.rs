pub use failure::{err_msg, Fail, Fallible, ResultExt};

macro_rules! to_failure_compat {
    ($($error:path,)*) => {
        pub trait ToFailureCompat<T> {
            fn to_failure(self) -> T;
        }

        $(
            impl<T> ToFailureCompat<Fallible<T>> for Result<T, $error> {
                fn to_failure(self) -> Fallible<T> {
                    match self {
                        Ok(ok) => Ok(ok),
                        Err(err) => Err(err_msg(format!("{}", err))),
                    }
                }
            }

            impl ToFailureCompat<::failure::Error> for $error {
                fn to_failure(self) -> ::failure::Error {
                    err_msg(format!("{}", self)).into()
                }
            }
        )*
    }
}

to_failure_compat! {
    ::crates_index::Error,
    ::tera::Error,
}
