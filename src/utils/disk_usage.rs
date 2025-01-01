use crate::prelude::*;
use std::path::Path;
use systemstat::{Filesystem, Platform, System};

pub(crate) struct DiskUsage {
    mount_point: String,
    usage: f32,
}

impl DiskUsage {
    pub(crate) fn fetch() -> Fallible<Self> {
        let fs = current_mount()?;
        Ok(Self {
            mount_point: fs.fs_mounted_on.clone(),
            usage: (fs.total.as_u64() - fs.free.as_u64()) as f32 / fs.total.as_u64() as f32,
        })
    }

    pub(crate) fn is_threshold_reached(&self, threshold: f32) -> bool {
        let usage = (self.usage * 100.0) as u8;
        if self.usage < threshold {
            info!("{} disk usage at {}%", self.mount_point, usage);
            false
        } else {
            warn!(
                "{} disk usage at {}%, which is over the threshold of {}%",
                self.mount_point,
                usage,
                (threshold * 100.0) as u8
            );
            true
        }
    }
}

fn current_mount() -> Fallible<Filesystem> {
    let current_dir = crate::utils::path::normalize_path(&crate::dirs::WORK_DIR);
    let system = System::new();

    let mut found = None;
    let mut found_pos = usize::MAX;
    for mount in system.mounts()?.into_iter() {
        let path = Path::new(&mount.fs_mounted_on);
        for (i, ancestor) in current_dir.ancestors().enumerate() {
            if ancestor == path && i < found_pos {
                found_pos = i;
                found = Some(mount);
                break;
            }
        }
    }
    found.ok_or_else(|| anyhow!("failed to find the current mount"))
}
