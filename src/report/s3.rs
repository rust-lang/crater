use errors::*;
use mime::Mime;
use report::ReportWriter;
use rusoto_core::{DefaultCredentialsProvider, Region, default_tls_client};
use rusoto_s3::{PutObjectRequest, S3, S3Client};
use std::borrow::Cow;
use std::fmt::{self, Display};
use std::io;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use uri::Uri;

#[derive(Debug, Clone)]
pub struct S3Prefix {
    pub bucket: String,
    pub prefix: PathBuf,
}

impl FromStr for S3Prefix {
    type Err = Error;
    fn from_str(uri: &str) -> Result<S3Prefix> {
        match Uri::new(uri).chain_err(|| ErrorKind::BadS3Uri)? {
            Uri {
                scheme,
                username: None,
                password: None,
                host: Some(bucket),
                port: None,
                path: prefix,
                query: None,
                fragment: None,
            } => {
                if scheme == "s3" {
                    Ok(S3Prefix {
                           bucket,
                           prefix: match prefix {
                               Some(prefix) => prefix[1..].into(),
                               None => PathBuf::new(),
                           },
                       })
                } else {
                    Err(ErrorKind::BadS3Uri.into())
                }
            }
            _ => Err(ErrorKind::BadS3Uri.into()),
        }
    }
}


pub struct S3Writer {
    prefix: S3Prefix,
    client: Box<S3>,
}

impl S3Writer {
    pub fn create(prefix: S3Prefix) -> S3Writer {
        let credentials = DefaultCredentialsProvider::new().unwrap();
        let client = S3Client::new(default_tls_client().unwrap(), credentials, Region::UsEast1);

        S3Writer {
            prefix,
            client: Box::new(client),
        }
    }

    fn write_vec<P: AsRef<Path>>(&self, path: P, s: Vec<u8>, mime: &Mime) -> Result<()> {
        self.client
            .put_object(&PutObjectRequest {
                            acl: Some("public-read".into()),
                            body: Some(s),
                            bucket: self.prefix.bucket.clone(),
                            key: self.prefix.prefix.join(path).to_string_lossy().into(),
                            content_type: Some(mime.to_string()),
                            ..Default::default()
                        })
            .chain_err(|| "S3")?;
        Ok(())
    }
}

impl ReportWriter for S3Writer {
    fn write_string<P: AsRef<Path>>(&self, path: P, s: Cow<str>, mime: &Mime) -> Result<()> {
        self.write_vec(path, s.into_owned().into_bytes(), mime)
    }
    fn copy<P: AsRef<Path>, R: io::Read>(&self, r: &mut R, path: P, mime: &Mime) -> Result<()> {
        let mut bytes = Vec::new();
        io::copy(r, &mut bytes)?;
        self.write_vec(path, bytes, mime)
    }
}

impl Display for S3Prefix {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        format_args!("s3://{}/{}", self.bucket, self.prefix.display()).fmt(f)
    }
}

impl Display for S3Writer {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.prefix.fmt(f)
    }
}
