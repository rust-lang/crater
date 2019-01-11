#[macro_use]
mod driver;

minicrater! {
    single_thread_small {
        ex: "small",
        crate_select: "demo",
        multithread: false,
        ignore_blacklist: false,
    },

    single_thread_full {
        ex: "full",
        crate_select: "local",
        multithread: false,
        ignore_blacklist: false,
    },

    single_thread_blacklist {
        ex: "blacklist",
        crate_select: "demo",
        multithread: false,
        ignore_blacklist: false,
    },

    single_thread_ignore_blacklist {
        ex: "ignore-blacklist",
        crate_select: "demo",
        multithread: false,
        ignore_blacklist: true,
    },

    multi_thread_full {
        ex: "full",
        crate_select: "local",
        multithread: true,
        ignore_blacklist: false,
    },
}
