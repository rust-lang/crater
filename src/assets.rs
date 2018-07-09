use errors::*;
use file;
use mime::{self, Mime};
use serde::Serialize;
use std::borrow::Cow;
use std::collections::HashMap;
use std::path::PathBuf;
use tera::Tera;

#[cfg(not(debug_assertions))]
lazy_static! {
    static ref TERA_CACHE: Tera = match build_tera_cache() {
        Ok(tera) => tera,
        Err(err) => {
            ::util::report_error(&err);
            ::std::process::exit(1);
        }
    };
}

macro_rules! load_files {
    (templates: [$($template:expr,)*], assets: [$($asset:expr => $mime:ident,)*],) => {
        lazy_static! {
            static ref ASSETS: HashMap<&'static str, Asset> = {
                let mut assets = HashMap::new();
                $(
                    let content = load_files!(_content concat!("assets/", $asset));
                    assets.insert($asset, Asset {
                        content,
                        mime: mime::$mime,
                    });
                )*
                assets
            };

            static ref TEMPLATES: HashMap<&'static str, FileContent> = {
                let mut templates = HashMap::new();
                $(templates.insert($template, load_files!(_content concat!("templates/", $template)));)*
                templates
            };
        }
    };

    (_content $file:expr) => {{
        #[cfg(debug_assertions)]
        {
            warn!("loaded dynamic asset (use release builds to statically bundle it): {}", $file);
            FileContent::Dynamic($file.into())
        }

        #[cfg(not(debug_assertions))]
        {
            FileContent::Static(include_str!(concat!("../", $file)))
        }
    }};
}

load_files! {
    templates: [
        "macros.html",
        "report.html",
    ],
    assets: [
        "report.css" => TEXT_CSS,
        "report.js" => TEXT_JAVASCRIPT,
    ],
}

enum FileContent {
    #[cfg_attr(debug_assertions, allow(dead_code))]
    Static(&'static str),
    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    Dynamic(PathBuf),
}

impl FileContent {
    fn load(&self) -> Result<Cow<str>> {
        Ok(match *self {
            FileContent::Static(content) => Cow::Borrowed(content),
            FileContent::Dynamic(ref path) => Cow::Owned(file::read_string(path)?),
        })
    }
}

pub struct Asset {
    content: FileContent,
    mime: Mime,
}

impl Asset {
    pub fn content(&self) -> Result<Cow<str>> {
        self.content.load()
    }

    pub fn mime(&self) -> &Mime {
        &self.mime
    }
}

pub fn load(name: &str) -> Result<&Asset> {
    if let Some(ref asset) = ASSETS.get(name) {
        Ok(asset)
    } else {
        bail!(
            "unknown static file (did you add it to src/assets.rs?): {}",
            name
        );
    }
}

fn build_tera_cache() -> Result<Tera> {
    let mut templates = Vec::new();
    for (name, content) in TEMPLATES.iter() {
        templates.push((*name, content.load()?));
    }

    let to_add = templates
        .iter()
        .map(|(n, c)| (*n, c as &str))
        .collect::<Vec<_>>();

    let mut tera = Tera::default();
    tera.add_raw_templates(to_add)?;
    Ok(tera)
}

pub fn render_template<C: Serialize>(name: &str, context: &C) -> Result<String> {
    // On debug builds the cache is rebuilt every time to pick up changed templates
    let tera_owned;
    let tera;

    #[cfg(debug_assertions)]
    {
        tera_owned = build_tera_cache()?;
        tera = &tera_owned;
    }

    #[cfg(not(debug_assertions))]
    {
        tera = &TERA_CACHE;
    }

    Ok(tera.render(name, context)?)
}
