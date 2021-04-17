use libc::c_int;
use std::fs::OpenOptions;
use std::io;
use std::os::unix::io::AsRawFd;
use std::os::unix::process::CommandExt;

fn main() {
    let read_end = std::thread::spawn(|| {
        OpenOptions::new()
            .read(true)
            .open("/tmp/crater-runner-fifo")
            .unwrap()
    });
    let write_end = std::thread::spawn(|| {
        OpenOptions::new()
            .write(true)
            .open("/tmp/crater-runner-fifo")
            .unwrap()
    });

    let read_end = read_end.join().unwrap();
    let write_end = write_end.join().unwrap();

    let read_fd = read_end.as_raw_fd();
    let write_fd = write_end.as_raw_fd();

    let mut args = std::env::args_os();
    let _ = args.next(); // self
    let mut cmd = std::process::Command::new(args.next().unwrap());

    let arg = format!("{},{} -j", read_fd, write_fd);
    cmd.env(
        "CARGO_MAKEFLAGS",
        format!("--jobserver-fds={0} --jobserver-auth={0}", arg),
    );

    cmd.args(args);
    unsafe {
        cmd.pre_exec(move || {
            set_cloexec(read_fd, false)?;
            set_cloexec(write_fd, false)?;
            Ok(())
        });
    }
    let mut child = cmd.spawn().unwrap();

    let status = child.wait().unwrap();
    std::process::exit(status.code().unwrap());
}

fn set_cloexec(fd: c_int, set: bool) -> io::Result<()> {
    unsafe {
        let previous = cvt(libc::fcntl(fd, libc::F_GETFD))?;
        let new = if set {
            previous | libc::FD_CLOEXEC
        } else {
            previous & !libc::FD_CLOEXEC
        };
        if new != previous {
            cvt(libc::fcntl(fd, libc::F_SETFD, new))?;
        }
        Ok(())
    }
}

fn cvt(t: c_int) -> io::Result<c_int> {
    if t == -1 {
        Err(io::Error::last_os_error())
    } else {
        Ok(t)
    }
}
