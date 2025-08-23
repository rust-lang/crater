use crate::prelude::*;

pub(crate) struct DiskUsage {
    usage: f32,
}

impl DiskUsage {
    pub(crate) fn fetch() -> Fallible<Self> {
        let stat = nix::sys::statvfs::statvfs(&crate::utils::path::normalize_path(
            &crate::dirs::WORK_DIR,
        ))?;
        Ok(Self {
            usage: stat.blocks_available() as f32 / stat.blocks() as f32,
        })
    }

    pub(crate) fn is_threshold_reached(&self, threshold: f32) -> bool {
        let usage = (self.usage * 100.0) as u8;
        if self.usage < threshold {
            info!("disk usage at {}%", usage);
            false
        } else {
            warn!(
                "disk usage at {}%, which is over the threshold of {}%",
                usage,
                (threshold * 100.0) as u8
            );
            true
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    #[cfg(unix)]
    fn check() {
        let usage = DiskUsage::fetch().unwrap();
        // Make sure usage is in a reasonable range.
        assert!(usage.usage > 0.05, "{}", usage.usage);
        assert!(usage.usage < 0.95, "{}", usage.usage);
    }
}
