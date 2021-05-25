use crate::prelude::*;
use std::path::Path;
use systemstat::{ByteSize, Filesystem, Platform, System};

pub(crate) struct DiskUsage {
    mount_point: String,
    usage: f32,
    free: ByteSize,
}

impl DiskUsage {
    pub(crate) fn fetch() -> Fallible<Self> {
        let fs = current_mount()?;
        Ok(Self {
            mount_point: fs.fs_mounted_on.clone(),
            usage: (fs.total.as_u64() - fs.free.as_u64()) as f32 / fs.total.as_u64() as f32,
            free: fs.free,
        })
    }

    pub(crate) fn has_gigabytes_left(&self, free_space: u32) -> bool {
        let usage = (self.usage * 100.0) as u8;
        if self.free < ByteSize::gb(free_space as u64) {
            info!(
                "{} disk usage at {}%: {} free",
                self.mount_point,
                usage,
                self.free.to_string_as(false)
            );
            false
        } else {
            warn!(
                "{} disk usage at {}%: {} free which is less than {} GB free",
                self.mount_point,
                usage,
                self.free.to_string_as(false),
                free_space,
            );
            true
        }
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
    let mut found_pos = std::usize::MAX;
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
    found.ok_or_else(|| failure::err_msg("failed to find the current mount"))
}
