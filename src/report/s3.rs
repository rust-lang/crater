use errors::*;
use mime::Mime;
use report::ReportWriter;
use rusoto_core::{default_tls_client, DefaultCredentialsProvider, Region};
use rusoto_s3::{GetBucketLocationRequest, PutObjectRequest, S3, S3Client};
use std::borrow::Cow;
use std::fmt::{self, Display};
use std::io;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::thread;
use std::time::Duration;
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
            } => if scheme == "s3" {
                Ok(S3Prefix {
                    bucket,
                    prefix: match prefix {
                        Some(prefix) => prefix[1..].into(),
                        None => PathBuf::new(),
                    },
                })
            } else {
                Err(ErrorKind::BadS3Uri.into())
            },
            _ => Err(ErrorKind::BadS3Uri.into()),
        }
    }
}

pub struct S3Writer {
    prefix: S3Prefix,
    client: Box<S3>,
}

fn get_client_for_bucket(bucket: &str) -> Result<Box<S3>> {
    let make_client = |region| {
        let credentials = DefaultCredentialsProvider::new().unwrap();
        S3Client::new(default_tls_client().unwrap(), credentials, region)
    };
    let client = make_client(Region::UsEast1);
    let response = client
        .get_bucket_location(&GetBucketLocationRequest {
            bucket: bucket.into(),
        })
        .chain_err(|| "S3 failure to get bucket location")?;
    let region = match response.location_constraint.as_ref() {
        Some(region) if region == "" => Region::UsEast1,
        Some(region) => region.parse().chain_err(|| "Unknown bucket region.")?,
        None => bail!{"Couldn't determine bucket region"},
    };

    Ok(Box::new(make_client(region)))
}

const S3RETRIES: u64 = 4;

impl S3Writer {
    pub fn create(prefix: S3Prefix) -> Result<S3Writer> {
        let client = get_client_for_bucket(&prefix.bucket)?;

        Ok(S3Writer { prefix, client })
    }

    fn write_vec<P: AsRef<Path>>(&self, path: P, s: Vec<u8>, mime: &Mime) -> Result<()> {
        let mut retry = 0;
        let req = PutObjectRequest {
            acl: Some("public-read".into()),
            body: Some(s),
            bucket: self.prefix.bucket.clone(),
            key: self.prefix
                .prefix
                .join(path.as_ref())
                .to_string_lossy()
                .into(),
            content_type: Some(mime.to_string()),
            ..Default::default()
        };
        loop {
            match self.client.put_object(&req) {
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
                    return r.map(|_| ())
                        .chain_err(|| format!("S3 failure to upload {:?}", path.as_ref()))
                }
            }
        }
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
