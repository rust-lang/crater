use crate::prelude::*;
use crate::report::ReportWriter;
use crate::results::EncodingType;
use mime::Mime;
use rusoto_core::request::HttpClient;
use rusoto_core::{DefaultCredentialsProvider, Region};
use rusoto_s3::{GetBucketLocationRequest, PutObjectRequest, S3Client, S3};
use std::borrow::Cow;
use std::fmt::{self, Display};
use std::io;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::thread;
use std::time::Duration;
use url::{Host, Url};

#[derive(Debug, Fail)]
pub enum S3Error {
    #[fail(display = "bad S3 url: {}", _0)]
    BadUrl(String),
    #[fail(display = "unknown bucket region")]
    UnknownBucketRegion,
}

#[derive(Debug, Clone)]
#[cfg_attr(test, derive(PartialEq, Eq))]
pub struct S3Prefix {
    pub bucket: String,
    pub prefix: PathBuf,
}

impl FromStr for S3Prefix {
    type Err = ::failure::Error;

    fn from_str(url: &str) -> Fallible<S3Prefix> {
        let parsed = Url::parse(url).with_context(|_| S3Error::BadUrl(url.into()))?;

        if parsed.scheme() != "s3"
            || parsed.username() != ""
            || parsed.password().is_some()
            || parsed.port().is_some()
            || parsed.query().is_some()
            || parsed.fragment().is_some()
        {
            return Err(S3Error::BadUrl(url.into()).into());
        }

        let bucket = if let Some(Host::Domain(host)) = parsed.host() {
            host.to_string()
        } else {
            return Err(S3Error::BadUrl(url.into()).into());
        };

        Ok(S3Prefix {
            bucket,
            prefix: parsed.path()[1..].into(),
        })
    }
}

pub struct S3Writer {
    prefix: S3Prefix,
    client: Box<dyn S3>,
}

pub fn get_client_for_bucket(bucket: &str) -> Fallible<Box<dyn S3>> {
    let make_client = |region| -> Fallible<S3Client> {
        let credentials = DefaultCredentialsProvider::new().unwrap();
        Ok(S3Client::new_with(HttpClient::new()?, credentials, region))
    };
    let client = make_client(Region::UsEast1)?;
    let response = client
        .get_bucket_location(GetBucketLocationRequest {
            bucket: bucket.into(),
        })
        .sync()
        .context(S3Error::UnknownBucketRegion)?;
    let region = match response.location_constraint.as_ref() {
        Some(region) if region == "" => Region::UsEast1,
        Some(region) => Region::from_str(region).context(S3Error::UnknownBucketRegion)?,
        None => return Err(S3Error::UnknownBucketRegion.into()),
    };

    Ok(Box::new(make_client(region)?))
}

const S3RETRIES: u64 = 4;

impl S3Writer {
    pub fn create(client: Box<dyn S3>, prefix: S3Prefix) -> Fallible<S3Writer> {
        Ok(S3Writer { prefix, client })
    }
}

impl ReportWriter for S3Writer {
    fn write_bytes<P: AsRef<Path>>(
        &self,
        path: P,
        s: Vec<u8>,
        mime: &Mime,
        encoding_type: EncodingType,
    ) -> Fallible<()> {
        let mut retry = 0;
        loop {
            let req = PutObjectRequest {
                acl: Some("public-read".into()),
                body: Some(s.clone().into()),
                bucket: self.prefix.bucket.clone(),
                key: self
                    .prefix
                    .prefix
                    .join(path.as_ref())
                    .to_string_lossy()
                    .into(),
                content_type: Some(mime.to_string()),
                content_encoding: match encoding_type {
                    EncodingType::Plain => None,
                    EncodingType::Gzip => Some("gzip".into()),
                },
                ..Default::default()
            };
            match self.client.put_object(req).sync() {
                Err(_) if retry < S3RETRIES => {
                    retry += 1;
                    thread::sleep(Duration::from_secs(2 * retry));
                    warn!(
                        "retry ({}/{}) S3 put to {:?}",
                        retry,
                        S3RETRIES,
                        path.as_ref()
                    );
                    continue;
                }
                r => {
                    if let Err(::rusoto_s3::PutObjectError::Unknown(ref resp)) = r {
                        error!("S3 request status: {}", resp.status);
                        error!("S3 request body: {}", String::from_utf8_lossy(&resp.body));
                        error!("S3 request headers: {:?}", resp.headers);
                    }
                    r.with_context(|_| format!("S3 failure to upload {:?}", path.as_ref()))?;
                    return Ok(());
                }
            }
        }
    }

    fn write_string<P: AsRef<Path>>(&self, path: P, s: Cow<str>, mime: &Mime) -> Fallible<()> {
        self.write_bytes(path, s.into_owned().into_bytes(), mime, EncodingType::Plain)
    }

    fn copy<P: AsRef<Path>, R: io::Read>(&self, r: &mut R, path: P, mime: &Mime) -> Fallible<()> {
        let mut bytes = Vec::new();
        io::copy(r, &mut bytes)?;
        self.write_bytes(path, bytes, mime, EncodingType::Plain)
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

#[cfg(test)]
mod tests {
    use super::S3Prefix;
    use std::str::FromStr;

    #[test]
    fn test_parse_s3prefix() {
        assert_eq!(
            S3Prefix::from_str("s3://bucket-name").unwrap(),
            S3Prefix {
                bucket: "bucket-name".into(),
                prefix: "".into(),
            }
        );
        assert_eq!(
            S3Prefix::from_str("s3://bucket-name/path/prefix").unwrap(),
            S3Prefix {
                bucket: "bucket-name".into(),
                prefix: "path/prefix".into(),
            }
        );

        for bad in &[
            "https://example.com",
            "s3://user:pass@bucket/path/prefix",
            "s3://bucket:80",
            "s3://bucket/path/prefix?query#fragment",
        ] {
            assert!(S3Prefix::from_str(bad).is_err(), "valid bad url: {}", bad);
        }
    }
}
