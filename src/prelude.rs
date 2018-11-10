use failure::Context;
pub use failure::{err_msg, Fail, Fallible, ResultExt};

pub trait FailExt {
    fn downcast_ctx<T: Fail>(&self) -> Option<&T>;
}

impl FailExt for dyn Fail {
    fn downcast_ctx<T: Fail>(&self) -> Option<&T> {
        if let Some(res) = self.downcast_ref::<T>() {
            Some(res)
        } else if let Some(ctx) = self.downcast_ref::<Context<T>>() {
            Some(ctx.get_context())
        } else {
            None
        }
    }
}

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
