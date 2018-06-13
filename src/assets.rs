use errors::*;
use file;
use handlebars::Handlebars;
use mime::{self, Mime};
use serde::Serialize;
use std::borrow::Cow;
use std::collections::HashMap;
use std::path::PathBuf;

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

pub fn render_template<C: Serialize>(name: &str, context: &C) -> Result<String> {
    if let Some(ref content) = TEMPLATES.get(name) {
        Handlebars::new()
            .template_render(&content.load()?, context)
            .chain_err(|| format!("failed to render template: {}", name))
    } else {
        bail!(
            "unknown template (did you add it to src/assets.rs?): {}",
            name
        );
    }
}
