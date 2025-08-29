use crate::prelude::*;

pub(crate) struct DiskUsage {
    usage: f32,
}

impl DiskUsage {
    pub(crate) fn fetch() -> Fallible<Self> {
        #[cfg(unix)]
        {
            let path = crate::utils::path::normalize_path(&crate::dirs::WORK_DIR);
            let stat = nix::sys::statvfs::statvfs(&path)?;
            let available = stat.blocks_available();
            let total = stat.blocks();
            info!("{available} / {total} blocks used in {path:?}");
            Ok(Self {
                usage: available as f32 / total as f32,
            })
        }
        #[cfg(not(unix))]
        {
            Ok(Self { usage: 0.0 })
        }
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
